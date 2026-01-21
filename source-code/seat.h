#ifndef SEAT_H
#define SEAT_H

#include <wayland-server-core.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_xcursor_manager.h>

struct gf_server;
struct gf_view;

struct gf_keyboard_group {
  struct gf_seat *seat;
  struct wlr_keyboard_group *wlr_group;
  bool is_virtual;
  struct wl_list link;
  struct wl_listener key;
  struct wl_listener modifiers;
  struct wl_listener destroy;
};

struct gf_pointer {
  struct gf_seat *seat;
  struct wlr_pointer *pointer;
  struct wl_list link;
  struct wl_listener destroy;
};

struct gf_touch {
  struct gf_seat *seat;
  struct wlr_touch *touch;
  struct wl_list link;
  struct wl_listener destroy;
};

struct gf_drag_icon {
  struct gf_seat *seat;
  struct wlr_drag_icon *wlr_drag_icon;
  struct wlr_scene_tree *scene_tree;
  struct wl_list link;
  struct wl_listener destroy;
};

struct gf_seat {
  struct gf_server *server;
  struct wlr_seat *seat;
  struct wlr_cursor *cursor;
  struct wlr_xcursor_manager *xcursor_manager;
  struct wl_list keyboard_groups;
  struct wl_list pointers;
  struct wl_list touch;
  struct wl_list drag_icons;
  struct wl_listener new_input;
  struct wl_listener cursor_motion_relative;
  struct wl_listener cursor_motion_absolute;
  struct wl_listener cursor_button;
  struct wl_listener cursor_axis;
  struct wl_listener cursor_frame;
  struct wl_listener touch_down;
  struct wl_listener touch_up;
  struct wl_listener touch_motion;
  struct wl_listener touch_frame;
  struct wl_listener request_start_drag;
  struct wl_listener start_drag;
  struct wl_listener request_set_cursor;
  struct wl_listener request_set_selection;
  struct wl_listener request_set_primary_selection;
  struct wl_listener destroy;
  int32_t touch_id;
  double touch_lx, touch_ly;
};

struct gf_seat *seat_create(struct gf_server *server, struct wlr_backend *backend);
struct gf_view *seat_get_focus(struct gf_seat *seat);
void seat_set_focus(struct gf_seat *seat, struct gf_view *view);
void seat_center_cursor(struct gf_seat *seat);

#endif /* SEAT_H */
