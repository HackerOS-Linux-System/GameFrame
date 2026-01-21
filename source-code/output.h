#ifndef OUTPUT_H
#define OUTPUT_H

#include <wayland-server-core.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_scene.h>

struct gf_server;
struct gf_output;

struct gf_output {
  struct wlr_output *wlr_output;
  struct gf_server *server;
  struct wl_listener destroy;
  struct wl_listener commit;
  struct wl_listener request_state;
  struct wl_listener frame;
  struct wl_list link;
  struct wlr_scene_output *scene_output;
};

void handle_new_output(struct wl_listener *listener, void *data);
void handle_output_layout_change(struct wl_listener *listener, void *data);
void handle_output_manager_apply(struct wl_listener *listener, void *data);
void handle_output_manager_test(struct wl_listener *listener, void *data);
void output_set_window_title(struct gf_output *output, const char *title);

#endif /* OUTPUT_H */
