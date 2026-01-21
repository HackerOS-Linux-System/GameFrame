#ifndef XWAYLAND_H
#define XWAYLAND_H

#include <wayland-server-core.h>
#include <wlr/types/wlr_xwayland.h>

struct gf_view;
struct gf_server;

struct gf_xwayland_view {
  struct gf_view view;
  struct wlr_xwayland_surface *xwayland_surface;
  struct wl_listener commit;
  struct wl_listener map;
  struct wl_listener unmap;
  struct wl_listener destroy;
  struct wl_listener request_configure;
  struct wl_listener associate;
  struct wl_listener dissociate;
};

struct gf_xwayland_view *xwayland_view_from_view(struct gf_view *view);
bool xwayland_view_should_manage(struct gf_view *view);
void handle_new_xwayland_surface(struct wl_listener *listener, void *data);

#endif /* XWAYLAND_H */
