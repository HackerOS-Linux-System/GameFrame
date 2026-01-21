#ifndef SERVER_H
#define SERVER_H

#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/types/wlr_allocator.h>
#include <wlr/types/wlr_idle.h>
#include <wlr/types/wlr_idle_inhibit_v1.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_renderer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_xdg_shell.h>

struct gf_seat;
struct gf_output;

enum gameframe_multi_output_mode {
  GAMEFRAME_MULTI_OUTPUT_MODE_EXTEND,
  GAMEFRAME_MULTI_OUTPUT_MODE_LAST,
};

struct gf_server {
  struct wl_display *wl_display;
  struct wl_listener display_destroy;
  bool terminated;
  struct wlr_backend *backend;
  struct wlr_session *session;
  struct wlr_renderer *renderer;
  struct wlr_allocator *allocator;
  struct wlr_output_layout *output_layout;
  struct wl_listener output_layout_change;
  struct wlr_scene *scene;
  struct wlr_scene_output_layout *scene_output_layout;
  struct wl_list views;
  struct wl_list outputs;
  struct gf_seat *seat;
  struct wlr_idle_notifier_v1 *idle;
  struct wlr_idle_inhibit_manager_v1 *idle_inhibit_v1;
  struct wl_listener new_idle_inhibitor_v1;
  struct wlr_xdg_shell *xdg_shell;
  struct wl_listener new_xdg_toplevel;
  struct wl_listener new_xdg_popup;
  struct wlr_xdg_decoration_manager_v1 *xdg_decoration_manager;
  struct wl_listener new_xdg_decoration;
  struct wlr_output_manager_v1 *output_manager_v1;
  struct wl_listener output_manager_apply;
  struct wl_listener output_manager_test;
  struct wlr_relative_pointer_manager_v1 *relative_pointer_manager;
  struct wlr_foreign_toplevel_manager_v1 *foreign_toplevel_manager;
  enum wlr_log_importance log_level;
  int nested_width, nested_height;
  int game_width, game_height;
  int fps_focused, fps_unfocused;
  bool borderless, fullscreen;
  bool xdg_decoration;
  enum gameframe_multi_output_mode output_mode;
  bool allow_vt_switch;
  bool return_app_code;
  struct wl_list inhibitors;
  struct wl_listener new_output;

#if GAMEFRAME_HAS_XWAYLAND
  struct wlr_xwayland *xwayland;
  struct wl_listener new_xwayland_surface;
#endif

#if GAMEFRAME_HAS_XWAYLAND
  struct wl_listener new_virtual_pointer;
#endif
};

void server_terminate(struct gf_server *server);

#endif /* SERVER_H */
