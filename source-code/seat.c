#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <wlr/backend.h>
#include <wlr/backend/multi.h>
#include <wlr/backend/session.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_idle.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_keyboard_group.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_primary_selection.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_server_decoration.h>
#include <wlr/types/wlr_session.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_tablet_v2.h>
#include <wlr/types/wlr_touch.h>
#include <wlr/types/wlr_virtual_keyboard_v1.h>
#include <wlr/types/wlr_virtual_pointer_v1.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/util/log.h>
#include <wlr/util/region.h>

#include "output.h"
#include "seat.h"
#include "server.h"
#include "view.h"
#if GAMEFRAME_HAS_XWAYLAND
#include "xwayland.h"
#endif

static void drag_icon_update_position(struct gf_drag_icon *drag_icon);

static struct gf_view *
desktop_view_at(struct gf_server *server, double lx, double ly, struct wlr_surface **surface, double *sx, double *sy)
{
	struct wlr_scene_node *node = wlr_scene_node_at(&server->scene->tree.node, lx, ly, sx, sy);
	if (node == NULL || node->type != WLR_SCENE_NODE_BUFFER) {
		return NULL;
	}

	struct wlr_scene_buffer *scene_buffer = wlr_scene_buffer_from_node(node);
	struct wlr_scene_surface *scene_surface = wlr_scene_surface_try_from_buffer(scene_buffer);
	if (!scene_surface) {
		return NULL;
	}

	*surface = scene_surface->surface;

	while (!node->data) {
		if (!node->parent) {
			node = NULL;
			break;
		}

		node = &node->parent->node;
	}

	assert(node != NULL);
	return node->data;
}

static void
press_cursor_button(struct gf_seat *seat, struct wlr_input_device *device, uint32_t time, uint32_t button,
		    uint32_t state, double lx, double ly)
{
	struct gf_server *server = seat->server;

	if (state == WLR_BUTTON_PRESSED) {
		double sx, sy;
		struct wlr_surface *surface;
		struct gf_view *view = desktop_view_at(server, lx, ly, &surface, &sx, &sy);
		struct gf_view *current = seat_get_focus(seat);
		if (view == current) {
			return;
		}

		if (view && !view_is_transient_for(current, view)) {
			seat_set_focus(seat, view);
		}
	}
}

static void
update_capabilities(struct gf_seat *seat)
{
	uint32_t caps = 0;

	if (!wl_list_empty(&seat->keyboard_groups)) {
		caps |= WL_SEAT_CAPABILITY_KEYBOARD;
	}
	if (!wl_list_empty(&seat->pointers)) {
		caps |= WL_SEAT_CAPABILITY_POINTER;
	}
	if (!wl_list_empty(&seat->touch)) {
		caps |= WL_SEAT_CAPABILITY_TOUCH;
	}
	wlr_seat_set_capabilities(seat->seat, caps);

	if ((caps & WL_SEAT_CAPABILITY_POINTER) == 0) {
		wlr_cursor_unset_image(seat->cursor);
	} else {
		wlr_cursor_set_xcursor(seat->cursor, seat->xcursor_manager, DEFAULT_XCURSOR);
	}
}

static void
map_input_device_to_output(struct gf_seat *seat, struct wlr_input_device *device, const char *output_name)
{
	if (!output_name) {
		wlr_log(WLR_INFO, "Input device %s cannot be mapped to an output device", device->name);
		return;
	}

	struct gf_output *output;
	wl_list_for_each (output, &seat->server->outputs, link) {
		if (strcmp(output_name, output->wlr_output->name) == 0) {
			wlr_log(WLR_INFO, "Mapping input device %s to output device %s", device->name,
				output->wlr_output->name);
			wlr_cursor_map_input_to_output(seat->cursor, device, output->wlr_output);
			return;
		}
	}

	wlr_log(WLR_INFO, "Couldn't map input device %s to an output", device->name);
}

static void
handle_touch_destroy(struct wl_listener *listener, void *data)
{
	struct gf_touch *touch = wl_container_of(listener, touch, destroy);
	struct gf_seat *seat = touch->seat;

	wl_list_remove(&touch->link);
	wlr_cursor_detach_input_device(seat->cursor, &touch->touch->base);
	wl_list_remove(&touch->destroy.link);
	free(touch);

	update_capabilities(seat);
}

static void
handle_new_touch(struct gf_seat *seat, struct wlr_touch *wlr_touch)
{
	struct gf_touch *touch = calloc(1, sizeof(struct gf_touch));
	if (!touch) {
		wlr_log(WLR_ERROR, "Cannot allocate touch");
		return;
	}

	touch->seat = seat;
	touch->touch = wlr_touch;
	wlr_cursor_attach_input_device(seat->cursor, &wlr_touch->base);

	wl_list_insert(&seat->touch, &touch->link);
	touch->destroy.notify = handle_touch_destroy;
	wl_signal_add(&wlr_touch->base.events.destroy, &touch->destroy);

	map_input_device_to_output(seat, &wlr_touch->base, wlr_touch->output_name);
	update_capabilities(seat);
}

static void
handle_pointer_destroy(struct wl_listener *listener, void *data)
{
	struct gf_pointer *pointer = wl_container_of(listener, pointer, destroy);
	struct gf_seat *seat = pointer->seat;

	wl_list_remove(&pointer->link);
	wlr_cursor_detach_input_device(seat->cursor, &pointer->pointer->base);
	wl_list_remove(&pointer->destroy.link);
	free(pointer);

	update_capabilities(seat);
}

static void
handle_new_pointer(struct gf_seat *seat, struct wlr_pointer *wlr_pointer)
{
	struct gf_pointer *pointer = calloc(1, sizeof(struct gf_pointer));
	if (!pointer) {
		wlr_log(WLR_ERROR, "Cannot allocate pointer");
		return;
	}

	pointer->seat = seat;
	pointer->pointer = wlr_pointer;
	wlr_cursor_attach_input_device(seat->cursor, &wlr_pointer->base);

	wl_list_insert(&seat->pointers, &pointer->link);
	pointer->destroy.notify = handle_pointer_destroy;
	wl_signal_add(&wlr_pointer->base.events.destroy, &pointer->destroy);

	map_input_device_to_output(seat, &wlr_pointer->base, wlr_pointer->output_name);
	update_capabilities(seat);
}

static void
handle_virtual_pointer(struct wl_listener *listener, void *data)
{
	struct gf_server *server = wl_container_of(listener, server, new_virtual_pointer);
	struct gf_seat *seat = server->seat;
	struct wlr_virtual_pointer_v1_new_pointer_event *event = data;
	struct wlr_virtual_pointer_v1 *pointer = event->new_pointer;
	struct wlr_pointer *wlr_pointer = &pointer->pointer;

	if (event->suggested_output != NULL) {
		wlr_pointer->output_name = strdup(event->suggested_output->name);
	}
	handle_new_pointer(seat, wlr_pointer);
	update_capabilities(seat);
}

static void
handle_modifier_event(struct wlr_keyboard *keyboard, struct gf_seat *seat)
{
	wlr_seat_set_keyboard(seat->seat, keyboard);
	wlr_seat_keyboard_notify_modifiers(seat->seat, &keyboard->modifiers);

	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static bool
handle_keybinding(struct gf_server *server, xkb_keysym_t sym)
{
	if (sym == XKB_KEY_Escape) {
		server_terminate(server);
		return true;
	}
	if (server->allow_vt_switch && sym >= XKB_KEY_XF86Switch_VT_1 && sym <= XKB_KEY_XF86Switch_VT_12) {
		if (wlr_backend_is_multi(server->backend)) {
			if (server->session) {
				unsigned vt = sym - XKB_KEY_XF86Switch_VT_1 + 1;
				wlr_session_change_vt(server->session, vt);
			}
		}
	} else {
		return false;
	}
	wlr_idle_notifier_v1_notify_activity(server->idle, server->seat->seat);
	return true;
}

static void
handle_key_event(struct wlr_keyboard *keyboard, struct gf_seat *seat, void *data)
{
	struct wlr_keyboard_key_event *event = data;

	xkb_keycode_t keycode = event->keycode + 8;

	const xkb_keysym_t *syms;
	int nsyms = xkb_state_key_get_syms(keyboard->xkb_state, keycode, &syms);

	bool handled = false;
	uint32_t modifiers = wlr_keyboard_get_modifiers(keyboard);
	if ((modifiers & WLR_MODIFIER_ALT) && event->state == WL_KEYBOARD_KEY_STATE_PRESSED) {
		for (int i = 0; i < nsyms; i++) {
			handled = handle_keybinding(seat->server, syms[i]);
		}
	}

	if (!handled) {
		wlr_seat_set_keyboard(seat->seat, keyboard);
		wlr_seat_keyboard_notify_key(seat->seat, event->time_msec, event->keycode, event->state);
	}

	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_keyboard_group_key(struct wl_listener *listener, void *data)
{
	struct gf_keyboard_group *cg_group = wl_container_of(listener, cg_group, key);
	handle_key_event(&cg_group->wlr_group->keyboard, cg_group->seat, data);
}

static void
handle_keyboard_group_modifiers(struct wl_listener *listener, void *data)
{
	struct gf_keyboard_group *group = wl_container_of(listener, group, modifiers);
	handle_modifier_event(&group->wlr_group->keyboard, group->seat);
}

static void
gf_keyboard_group_add(struct wlr_keyboard *keyboard, struct gf_seat *seat, bool virtual)
{
	if (!virtual) {
		struct gf_keyboard_group *group;
		wl_list_for_each (group, &seat->keyboard_groups, link) {
			if (group->is_virtual)
				continue;
			struct wlr_keyboard_group *wlr_group = group->wlr_group;
			if (wlr_keyboard_group_add_keyboard(wlr_group, keyboard)) {
				wlr_log(WLR_DEBUG, "Added new keyboard to existing group");
				return;
			}
		}
	}

	struct gf_keyboard_group *cg_group = calloc(1, sizeof(struct gf_keyboard_group));
	if (cg_group == NULL) {
		wlr_log(WLR_ERROR, "Failed to allocate keyboard group.");
		return;
	}
	cg_group->seat = seat;
	cg_group->is_virtual = virtual;
	cg_group->wlr_group = wlr_keyboard_group_create();
	if (cg_group->wlr_group == NULL) {
		wlr_log(WLR_ERROR, "Failed to create wlr keyboard group.");
		free(cg_group);
		return;
	}

	cg_group->wlr_group->data = cg_group;
	wlr_keyboard_set_keymap(&cg_group->wlr_group->keyboard, keyboard->keymap);
	wlr_keyboard_set_repeat_info(&cg_group->wlr_group->keyboard, keyboard->repeat_info.rate, keyboard->repeat_info.delay);

	wl_list_insert(&seat->keyboard_groups, &cg_group->link);

	cg_group->key.notify = handle_keyboard_group_key;
	wl_signal_add(&cg_group->wlr_group->events.key, &cg_group->key);
	cg_group->modifiers.notify = handle_keyboard_group_modifiers;
	wl_signal_add(&cg_group->wlr_group->events.modifiers, &cg_group->modifiers);

	wlr_keyboard_group_add_keyboard(cg_group->wlr_group, keyboard);

	update_capabilities(seat);
}

static void
handle_keyboard_destroy(struct wl_listener *listener, void *data)
{
	struct gf_keyboard_group *group = wl_container_of(listener, group, destroy);

	wl_list_remove(&group->link);
	wl_list_remove(&group->key.link);
	wl_list_remove(&group->modifiers.link);
	wl_list_remove(&group->destroy.link);
	wlr_keyboard_group_destroy(group->wlr_group);
	free(group);

	update_capabilities(group->seat);
}

static void
handle_new_keyboard(struct gf_seat *seat, struct wlr_keyboard *wlr_keyboard)
{
	struct wlr_input_device *device = &wlr_keyboard->base;
	bool virtual = device->type == WLR_INPUT_DEVICE_VIRTUAL_KEYBOARD;

	gf_keyboard_group_add(wlr_keyboard, seat, virtual);
	update_capabilities(seat);

	map_input_device_to_output(seat, device, device->output_name);
}

static void
handle_new_input(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, new_input);
	struct wlr_input_device *device = data;

	switch (device->type) {
	case WLR_INPUT_DEVICE_KEYBOARD:
		handle_new_keyboard(seat, wlr_keyboard_from_input_device(device));
		break;
	case WLR_INPUT_DEVICE_POINTER:
		handle_new_pointer(seat, wlr_pointer_from_input_device(device));
		break;
	case WLR_INPUT_DEVICE_TOUCH:
		handle_new_touch(seat, wlr_touch_from_input_device(device));
		break;
	default:
		break;
	}
}

static void
handle_cursor_motion_relative(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, cursor_motion_relative);
	struct wlr_pointer_motion_event *event = data;

	wlr_cursor_move(seat->cursor, &event->pointer->base, event->delta_x, event->delta_y);
	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);

	press_cursor_button(seat, &event->pointer->base, event->time_msec, 0, WL_KEYBOARD_KEY_STATE_PRESSED, seat->cursor->x, seat->cursor->y);
}

static void
handle_cursor_motion_absolute(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, cursor_motion_absolute);
	struct wlr_pointer_motion_absolute_event *event = data;

	wlr_cursor_warp_absolute(seat->cursor, &event->pointer->base, event->x, event->y);
	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);

	press_cursor_button(seat, &event->pointer->base, event->time_msec, 0, WL_KEYBOARD_KEY_STATE_PRESSED, seat->cursor->x, seat->cursor->y);
}

static void
handle_cursor_button(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, cursor_button);
	struct wlr_pointer_button_event *event = data;

	wlr_seat_pointer_notify_button(seat->seat, event->time_msec, event->button, event->state);
	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_cursor_axis(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, cursor_axis);
	struct wlr_pointer_axis_event *event = data;

	wlr_seat_pointer_notify_axis(seat->seat, event->time_msec, event->orientation, event->delta, event->delta_discrete, event->source);
	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_cursor_frame(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, cursor_frame);

	wlr_seat_pointer_notify_frame(seat->seat);
}

static void
handle_touch_down(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, touch_down);
	struct wlr_touch_down_event *event = data;

	seat->touch_id = event->touch_id;
	seat->touch_lx = event->x;
	seat->touch_ly = event->y;

	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_touch_up(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, touch_up);

	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_touch_motion(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, touch_motion);
	struct wlr_touch_motion_event *event = data;

	if (event->touch_id == seat->touch_id) {
		seat->touch_lx = event->x;
		seat->touch_ly = event->y;
	}

	wlr_idle_notifier_v1_notify_activity(seat->server->idle, seat->seat);
}

static void
handle_touch_frame(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, touch_frame);

	wlr_seat_touch_notify_frame(seat->seat);
}

static void
handle_request_start_drag(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, request_start_drag);
	struct wlr_seat_request_start_drag_event *event = data;

	wlr_seat_start_drag(seat->seat, event->drag, event->serial);
}

static void
handle_start_drag(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, start_drag);
	struct wlr_drag *wlr_drag = data;

	struct gf_drag_icon *drag_icon = calloc(1, sizeof(*drag_icon));
	if (drag_icon == NULL) {
		wlr_log(WLR_ERROR, "Could not allocate drag icon");
		return;
	}

	drag_icon->seat = seat;
	drag_icon->wlr_drag_icon = wlr_drag->icon;
	wl_list_insert(&seat->drag_icons, &drag_icon->link);

	drag_icon->scene_tree = wlr_scene_drag_icon_create(&seat->server->scene->tree, wlr_drag->icon);
	if (drag_icon->scene_tree == NULL) {
		wlr_log(WLR_ERROR, "Could not create scene drag icon");
		free(drag_icon);
		return;
	}

	drag_icon->destroy.notify = handle_drag_icon_destroy;
	wl_signal_add(&wlr_drag->icon->events.destroy, &drag_icon->destroy);

	drag_icon_update_position(drag_icon);
}

static void
handle_drag_icon_destroy(struct wl_listener *listener, void *data)
{
	struct gf_drag_icon *drag_icon = wl_container_of(listener, drag_icon, destroy);
	wl_list_remove(&drag_icon->link);
	wl_list_remove(&drag_icon->destroy.link);
	free(drag_icon);
}

static void
drag_icon_update_position(struct gf_drag_icon *drag_icon)
{
	struct wlr_drag_icon *wlr_drag_icon = drag_icon->wlr_drag_icon;
	struct wlr_scene_node *node = &drag_icon->scene_tree->node;
	double x = drag_icon->seat->cursor->x + wlr_drag_icon->sx;
	double y = drag_icon->seat->cursor->y + wlr_drag_icon->sy;
	wlr_scene_node_set_position(node, x, y);
}

static void
handle_request_set_cursor(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, request_set_cursor);
	struct wlr_seat_pointer_request_set_cursor_event *event = data;

	wlr_seat_client *focused_client = seat->seat->pointer_state.focused_client;
	if (event->seat_client == focused_client) {
		wlr_cursor_set_surface(seat->cursor, event->surface, event->hotspot_x, event->hotspot_y);
	}
}

static void
handle_request_set_selection(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, request_set_selection);
	struct wlr_seat_request_set_selection_event *event = data;

	wlr_seat_set_selection(seat->seat, event->source, event->serial);
}

static void
handle_request_set_primary_selection(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, request_set_primary_selection);
	struct wlr_seat_request_set_primary_selection_event *event = data;

	wlr_seat_set_primary_selection(seat->seat, event->source, event->serial);
}

struct gf_seat *
seat_create(struct gf_server *server, struct wlr_backend *backend)
{
	struct gf_seat *seat = calloc(1, sizeof(*seat));
	if (!seat) {
		wlr_log(WLR_ERROR, "Cannot allocate seat");
		return NULL;
	}

	seat->server = server;
	seat->seat = wlr_seat_create(server->wl_display, "seat0");
	if (!seat->seat) {
		wlr_log(WLR_ERROR, "Cannot allocate seat");
		free(seat);
		return NULL;
	}

	seat->cursor = wlr_cursor_create();
	if (!seat->cursor) {
		wlr_log(WLR_ERROR, "Cannot allocate cursor");
		wlr_seat_destroy(seat->seat);
		free(seat);
		return NULL;
	}

	wlr_cursor_attach_output_layout(seat->cursor, server->output_layout);

	seat->xcursor_manager = wlr_xcursor_manager_create(NULL, XCURSOR_SIZE);
	if (!seat->xcursor_manager) {
		wlr_log(WLR_ERROR, "Cannot allocate xcursor manager");
		wlr_cursor_destroy(seat->cursor);
		wlr_seat_destroy(seat->seat);
		free(seat);
		return NULL;
	}

	wlr_cursor_set_xcursor(seat->cursor, seat->xcursor_manager, DEFAULT_XCURSOR);

	wl_list_init(&seat->keyboards);
	wl_list_init(&seat->keyboard_groups);
	wl_list_init(&seat->pointers);
	wl_list_init(&seat->touch);
	wl_list_init(&seat->drag_icons);

	seat->new_input.notify = handle_new_input;
	wl_signal_add(&backend->events.new_input, &seat->new_input);

	seat->cursor_motion_relative.notify = handle_cursor_motion_relative;
	wl_signal_add(&seat->cursor->events.motion, &seat->cursor_motion_relative);

	seat->cursor_motion_absolute.notify = handle_cursor_motion_absolute;
	wl_signal_add(&seat->cursor->events.motion_absolute, &seat->cursor_motion_absolute);

	seat->cursor_button.notify = handle_cursor_button;
	wl_signal_add(&seat->cursor->events.button, &seat->cursor_button);

	seat->cursor_axis.notify = handle_cursor_axis;
	wl_signal_add(&seat->cursor->events.axis, &seat->cursor_axis);

	seat->cursor_frame.notify = handle_cursor_frame;
	wl_signal_add(&seat->cursor->events.frame, &seat->cursor_frame);

	seat->touch_down.notify = handle_touch_down;
	wl_signal_add(&seat->cursor->events.touch_down, &seat->touch_down);

	seat->touch_up.notify = handle_touch_up;
	wl_signal_add(&seat->cursor->events.touch_up, &seat->touch_up);

	seat->touch_motion.notify = handle_touch_motion;
	wl_signal_add(&seat->cursor->events.touch_motion, &seat->touch_motion);

	seat->touch_frame.notify = handle_touch_frame;
	wl_signal_add(&seat->cursor->events.touch_frame, &seat->touch_frame);

	seat->request_start_drag.notify = handle_request_start_drag;
	wl_signal_add(&seat->seat->events.request_start_drag, &seat->request_start_drag);

	seat->start_drag.notify = handle_start_drag;
	wl_signal_add(&seat->seat->events.start_drag, &seat->start_drag);

	seat->request_set_cursor.notify = handle_request_set_cursor;
	wl_signal_add(&seat->seat->pointer_state.events.request_set_cursor, &seat->request_set_cursor);

	seat->request_set_selection.notify = handle_request_set_selection;
	wl_signal_add(&seat->seat->events.request_set_selection, &seat->request_set_selection);

	seat->request_set_primary_selection.notify = handle_request_set_primary_selection;
	wl_signal_add(&seat->seat->events.request_set_primary_selection, &seat->request_set_primary_selection);

	seat->destroy.notify = seat_destroy;
	wl_signal_add(&seat->seat->events.destroy, &seat->destroy);

	update_capabilities(seat);

	return seat;
}

void
seat_destroy(struct wl_listener *listener, void *data)
{
	struct gf_seat *seat = wl_container_of(listener, seat, destroy);

	wl_list_remove(&seat->destroy.link);
	wlr_seat_destroy(seat->seat);
	wlr_xcursor_manager_destroy(seat->xcursor_manager);
	wlr_cursor_destroy(seat->cursor);
	free(seat);
}

struct gf_view *
seat_get_focus(struct gf_seat *seat)
{
	struct wlr_seat *wlr_seat = seat->seat;
	struct wlr_surface *surface = wlr_seat->keyboard_state.focused_surface;
	if (surface == NULL) {
		return NULL;
	}
	return view_from_wlr_surface(surface);
}

void
seat_set_focus(struct gf_seat *seat, struct gf_view *view)
{
	struct gf_server *server = seat->server;
	struct wlr_surface *surface = view->wlr_surface;
	struct wlr_keyboard *keyboard = wlr_seat_get_keyboard(seat->seat);

	if (keyboard) {
		wlr_seat_keyboard_notify_enter(seat->seat, surface, keyboard->keycodes, keyboard->num_keycodes, &keyboard->modifiers);
	}

	view_activate(view, true);
}

void
seat_center_cursor(struct gf_seat *seat)
{
	struct wlr_box layout_box;
	wlr_output_layout_get_box(seat->server->output_layout, NULL, &layout_box);

	double x = layout_box.width / 2.0;
	double y = layout_box.height / 2.0;

	wlr_cursor_warp(seat->cursor, NULL, x, y);
}
