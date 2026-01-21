#ifndef VIEW_H
#define VIEW_H

#include <wayland-server-core.h>
#include <wlr/types/wlr_foreign_toplevel_manager_v1.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_surface.h>

struct gf_server;

enum gf_view_type {
  GAMEFRAME_XDG_SHELL_VIEW,
  GAMEFRAME_XWAYLAND_VIEW,
};

struct gf_view_impl {
  char *(*get_title)(struct gf_view *view);
  void (*get_geometry)(struct gf_view *view, int *width, int *height);
  bool (*is_primary)(struct gf_view *view);
  bool (*is_transient_for)(struct gf_view *child, struct gf_view *parent);
  void (*activate)(struct gf_view *view, bool activate);
  void (*maximize)(struct gf_view *view, int width, int height);
  void (*destroy)(struct gf_view *view);
  void (*close)(struct gf_view *view);
};

struct gf_view {
  struct gf_server *server;
  enum gf_view_type type;
  const struct gf_view_impl *impl;
  struct wlr_surface *wlr_surface;
  struct wlr_scene_tree *scene_tree;
  int lx, ly;
  struct wl_list link;
  struct wlr_foreign_toplevel_handle_v1 *foreign_toplevel_handle;
  struct wl_listener request_activate;
  struct wl_listener request_close;
};

void view_init(struct gf_view *view, struct gf_server *server, enum gf_view_type type, const struct gf_view_impl *impl);
void view_destroy(struct gf_view *view);
void view_map(struct gf_view *view, struct wlr_surface *surface);
void view_unmap(struct gf_view *view);
void view_position(struct gf_view *view);
void view_position_all(struct gf_server *server);
char *view_get_title(struct gf_view *view);
bool view_is_primary(struct gf_view *view);
bool view_is_transient_for(struct gf_view *child, struct gf_view *parent);
void view_activate(struct gf_view *view, bool activate);
struct gf_view *view_from_wlr_surface(struct wlr_surface *surface);

#endif /* VIEW_H */
