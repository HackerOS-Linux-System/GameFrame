/*
 * GameFrame: A minimalist Wayland compositor inspired by Cage,
 * optimized for older GPUs. It uses wlroots and focuses on
 * basic functionality without relying on modern GPU features
 * like advanced shaders or high-performance rendering.
 *
 * This compositor creates a single fullscreen output and runs
 * a specified command (e.g., a terminal or game) inside it.
 * It supports basic input and output handling suitable for
 * legacy hardware.
 *
 * Build with: pkg-config --cflags --libs wlroots wayland-server xkbcommon libdrm
 * gcc -o gameframe gameframe.c `pkg-config --cflags --libs wlroots wayland-server xkbcommon libdrm`
 *
 * Run with: ./gameframe /path/to/your/app
 */
#include <assert.h>
#include <signal.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/render/allocator.h>
#include <wlr/render/wlr_renderer.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/util/log.h>
#include <wlr/types/wlr_cursor.h>

struct gameframe_server {
    struct wl_display *display;
    struct wlr_backend *backend;
    struct wlr_renderer *renderer;
    struct wlr_allocator *allocator;
    struct wlr_scene *scene;
    struct wlr_scene_output *scene_output;
    struct wlr_xdg_shell *xdg_shell;
    struct wlr_compositor *compositor;
    struct wlr_subcompositor *subcompositor;
    struct wlr_output_layout *output_layout;
    struct wlr_seat *seat;
    struct wlr_cursor *cursor;
    struct wl_listener new_output;
    struct wl_listener new_xdg_surface;
    struct wl_listener new_input;
    struct wl_listener request_set_cursor;
    struct wl_listener request_set_selection;
    pid_t child_pid;
};
struct gameframe_output {
    struct wlr_output *wlr_output;
    struct gameframe_server *server;
    struct wl_listener frame;
    struct wl_listener destroy;
};
struct gameframe_view {
    struct wlr_xdg_toplevel *xdg_toplevel;
    struct wlr_scene_tree *scene_tree;
    struct gameframe_server *server;
    struct wl_listener map;
    struct wl_listener unmap;
    struct wl_listener destroy;
    struct wl_listener request_move;
    struct wl_listener request_resize;
    struct wl_listener request_maximize;
    struct wl_listener request_fullscreen;
};
static void output_frame(struct wl_listener *listener, void *data) {
    struct gameframe_output *output = wl_container_of(listener, output, frame);
    struct wlr_scene *scene = output->server->scene;
    struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(scene, output->wlr_output);
    wlr_scene_output_commit(scene_output, NULL);
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    wlr_scene_output_send_frame_done(scene_output, &now);
}
static void output_destroy(struct wl_listener *listener, void *data) {
    struct gameframe_output *output = wl_container_of(listener, output, destroy);
    wl_list_remove(&output->frame.link);
    wl_list_remove(&output->destroy.link);
    free(output);
}
static void server_new_output(struct wl_listener *listener, void *data) {
    struct gameframe_server *server = wl_container_of(listener, server, new_output);
    struct wlr_output *wlr_output = data;
    wlr_output_init_render(wlr_output, server->allocator, server->renderer);
    struct wlr_output_state state;
    wlr_output_state_init(&state);
    wlr_output_state_set_enabled(&state, true);
    struct wlr_output_mode *mode = wlr_output_preferred_mode(wlr_output);
    if (mode != NULL) {
        wlr_output_state_set_mode(&state, mode);
    }
    wlr_output_commit_state(wlr_output, &state);
    wlr_output_state_finish(&state);
    struct gameframe_output *output = calloc(1, sizeof(*output));
    output->wlr_output = wlr_output;
    output->server = server;
    output->frame.notify = output_frame;
    wl_signal_add(&wlr_output->events.frame, &output->frame);
    output->destroy.notify = output_destroy;
    wl_signal_add(&wlr_output->events.destroy, &output->destroy);
    wlr_output_layout_add_auto(server->output_layout, wlr_output);
    server->scene_output = wlr_scene_output_create(server->scene, wlr_output);
}
static void xdg_toplevel_map(struct wl_listener *listener, void *data) {
    struct gameframe_view *view = wl_container_of(listener, view, map);
    wlr_scene_node_set_position(&view->scene_tree->node, 0, 0);
    wlr_xdg_toplevel_set_size(view->xdg_toplevel, 0, 0); // Fullscreen implicitly
    wlr_xdg_toplevel_set_fullscreen(view->xdg_toplevel, true);
}
static void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
    struct gameframe_view *view = wl_container_of(listener, view, unmap);
    // No-op for now
}
static void view_destroy(struct wl_listener *listener, void *data) {
    struct gameframe_view *view = wl_container_of(listener, view, destroy);
    wl_list_remove(&view->map.link);
    wl_list_remove(&view->unmap.link);
    wl_list_remove(&view->destroy.link);
    wl_list_remove(&view->request_move.link);
    wl_list_remove(&view->request_resize.link);
    wl_list_remove(&view->request_maximize.link);
    wl_list_remove(&view->request_fullscreen.link);
    free(view);
}
static void xdg_toplevel_request_move(struct wl_listener *listener, void *data) {
    // No moving in fullscreen
}
static void xdg_toplevel_request_resize(struct wl_listener *listener, void *data) {
    // No resizing in fullscreen
}
static void xdg_toplevel_request_maximize(struct wl_listener *listener, void *data) {
    // Already maximized/fullscreen
}
static void xdg_toplevel_request_fullscreen(struct wl_listener *listener, void *data) {
    struct gameframe_view *view = wl_container_of(listener, view, request_fullscreen);
    struct wlr_xdg_surface *xdg_surface = data;
    wlr_xdg_toplevel_set_fullscreen(view->xdg_toplevel, xdg_surface->toplevel->requested.fullscreen);
}
static void server_new_xdg_surface(struct wl_listener *listener, void *data) {
    struct gameframe_server *server = wl_container_of(listener, server, new_xdg_surface);
    struct wlr_xdg_surface *xdg_surface = data;
    if (xdg_surface->role != WLR_XDG_SURFACE_ROLE_TOPLEVEL) {
        return;
    }
    struct gameframe_view *view = calloc(1, sizeof(*view));
    view->server = server;
    view->xdg_toplevel = xdg_surface->toplevel;
    view->scene_tree = wlr_scene_xdg_surface_create(&server->scene->tree, xdg_surface);
    view->map.notify = xdg_toplevel_map;
    wl_signal_add(&xdg_surface->surface->events.map, &view->map);
    view->unmap.notify = xdg_toplevel_unmap;
    wl_signal_add(&xdg_surface->surface->events.unmap, &view->unmap);
    view->destroy.notify = view_destroy;
    wl_signal_add(&xdg_surface->events.destroy, &view->destroy);
    view->request_move.notify = xdg_toplevel_request_move;
    wl_signal_add(&view->xdg_toplevel->events.request_move, &view->request_move);
    view->request_resize.notify = xdg_toplevel_request_resize;
    wl_signal_add(&view->xdg_toplevel->events.request_resize, &view->request_resize);
    view->request_maximize.notify = xdg_toplevel_request_maximize;
    wl_signal_add(&view->xdg_toplevel->events.request_maximize, &view->request_maximize);
    view->request_fullscreen.notify = xdg_toplevel_request_fullscreen;
    wl_signal_add(&view->xdg_toplevel->events.request_fullscreen, &view->request_fullscreen);
}
static void process_keyboard(struct gameframe_server *server, struct wlr_keyboard *keyboard) {
    wlr_seat_set_keyboard(server->seat, keyboard);
    wlr_keyboard_set_repeat_info(keyboard, 25, 600);
}
static void server_new_input(struct wl_listener *listener, void *data) {
    struct gameframe_server *server = wl_container_of(listener, server, new_input);
    struct wlr_input_device *device = data;
    switch (device->type) {
    case WLR_INPUT_DEVICE_KEYBOARD: {
        struct wlr_keyboard *kb = wlr_keyboard_from_input_device(device);
        struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
        struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, NULL, XKB_KEYMAP_COMPILE_NO_FLAGS);
        wlr_keyboard_set_keymap(kb, keymap);
        xkb_keymap_unref(keymap);
        xkb_context_unref(context);
        process_keyboard(server, kb);
        break;
    }
    case WLR_INPUT_DEVICE_POINTER: {
        wlr_cursor_attach_input_device(server->cursor, device);
        break;
    }
    default:
        break;
    }
    uint32_t caps = WL_SEAT_CAPABILITY_POINTER | WL_SEAT_CAPABILITY_KEYBOARD;
    wlr_seat_set_capabilities(server->seat, caps);
}
static void request_set_cursor(struct wl_listener *listener, void *data) {
    struct gameframe_server *server = wl_container_of(listener, server, request_set_cursor);
    struct wlr_seat_pointer_request_set_cursor_event *event = data;
    struct wlr_seat_client *focused_client = server->seat->pointer_state.focused_client;
    if (focused_client != NULL && focused_client == event->seat_client) {
        wlr_cursor_set_surface(server->cursor, event->surface, event->hotspot_x, event->hotspot_y);
    }
}
static void request_set_selection(struct wl_listener *listener, void *data) {
    struct gameframe_server *server = wl_container_of(listener, server, request_set_selection);
    struct wlr_seat_request_set_selection_event *event = data;
    wlr_seat_set_selection(server->seat, event->source, event->serial);
}
int main(int argc, char *argv[]) {
    wlr_log_init(WLR_DEBUG, NULL);
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <command>\n", argv[0]);
        return 1;
    }
    struct gameframe_server server = {0};
    server.display = wl_display_create();
    if (server.display == NULL) {
        wlr_log(WLR_ERROR, "Cannot create wayland display");
        return 1;
    }
    server.backend = wlr_backend_autocreate(wl_display_get_event_loop(server.display), NULL);
    if (server.backend == NULL) {
        wlr_log(WLR_ERROR, "Cannot create backend");
        return 1;
    }
    server.renderer = wlr_renderer_autocreate(server.backend);
    if (server.renderer == NULL) {
        wlr_log(WLR_ERROR, "Cannot create renderer");
        return 1;
    }
    wlr_renderer_init_wl_display(server.renderer, server.display);
    server.allocator = wlr_allocator_autocreate(server.backend, server.renderer);
    if (server.allocator == NULL) {
        wlr_log(WLR_ERROR, "Cannot create allocator");
        return 1;
    }
    server.compositor = wlr_compositor_create(server.display, 5, server.renderer);
    server.subcompositor = wlr_subcompositor_create(server.display);
    wlr_data_device_manager_create(server.display);
    server.output_layout = wlr_output_layout_create(server.display);
    server.cursor = wlr_cursor_create();
    wlr_cursor_attach_output_layout(server.cursor, server.output_layout);
    server.scene = wlr_scene_create();
    server.new_output.notify = server_new_output;
    wl_signal_add(&server.backend->events.new_output, &server.new_output);
    server.xdg_shell = wlr_xdg_shell_create(server.display, 3);
    server.new_xdg_surface.notify = server_new_xdg_surface;
    wl_signal_add(&server.xdg_shell->events.new_surface, &server.new_xdg_surface);
    server.seat = wlr_seat_create(server.display, "seat0");
    server.request_set_cursor.notify = request_set_cursor;
    wl_signal_add(&server.seat->events.request_set_cursor, &server.request_set_cursor);
    server.request_set_selection.notify = request_set_selection;
    wl_signal_add(&server.seat->events.request_set_selection, &server.request_set_selection);
    server.new_input.notify = server_new_input;
    wl_signal_add(&server.backend->events.new_input, &server.new_input);
    const char *socket = wl_display_add_socket_auto(server.display);
    if (socket == NULL) {
        wlr_log(WLR_ERROR, "Cannot add socket");
        return 1;
    }
    if (!wlr_backend_start(server.backend)) {
        wlr_log(WLR_ERROR, "Cannot start backend");
        return 1;
    }
    setenv("WAYLAND_DISPLAY", socket, true);
    server.child_pid = fork();
    if (server.child_pid == 0) {
        execvp(argv[1], argv + 1);
        perror("execvp");
        exit(1);
    } else if (server.child_pid < 0) {
        wlr_log(WLR_ERROR, "Cannot fork");
        return 1;
    }
    wlr_log(WLR_INFO, "Running on WAYLAND_DISPLAY=%s", socket);
    wl_display_run(server.display);
    if (server.child_pid > 0) {
        kill(server.child_pid, SIGTERM);
        waitpid(server.child_pid, NULL, 0);
    }
    wl_display_destroy_clients(server.display);
    wl_display_destroy(server.display);
    wlr_scene_output_destroy(server.scene_output);
    wlr_output_layout_destroy(server.output_layout);
    wlr_allocator_destroy(server.allocator);
    wlr_renderer_destroy(server.renderer);
    wlr_backend_destroy(server.backend);
    return 0;
}
