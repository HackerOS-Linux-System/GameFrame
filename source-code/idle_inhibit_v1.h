#ifndef IDLE_INHIBIT_V1_H
#define IDLE_INHIBIT_V1_H

#include <wayland-server-core.h>
#include <wlr/types/wlr_idle_inhibit_v1.h>

struct gf_server;

struct gf_idle_inhibitor_v1 {
  struct gf_server *server;
  struct wl_list link; // server::inhibitors
  struct wl_listener destroy;
};

void handle_idle_inhibitor_v1_new(struct wl_listener *listener, void *data);

#endif /* IDLE_INHIBIT_V1_H */
