#include <stdlib.h>
#include <string.h>
#include <wlr/types/wlr_foreign_toplevel_manager_v1.h>
#include <wlr/util/log.h>

#include "server.h"
#include "view.h"
#include "xwayland.h"

struct gf_xwayland_view *
xwayland_view_from_view(struct gf_view *view)
{
	return (struct gf_xwayland_view *) view;
}

bool
xwayland_view_should_manage(struct gf_view *view)
{
	struct gf_xwayland_view *xwayland_view = xwayland_view_from_view(view);
	struct wlr_xwayland_surface *xwayland_surface = xwayland_view->xwayland_surface;
	return !xwayland_surface->override_redirect;
}

static char *
get_title(struct gf_view *view)
{
	struct gf_xwayland_view *xwayland_view = xwayland_view_from_view(view);
	return xwayland_view->xwayland_surface->title;
}

static void
get_geometry(struct gf_view *view, int *width_out, int *height_out)
{
	struct gf_xwayland_view *xwayland_view = xwayland_view_from_view(view);
	struct wlr_xwayland_surface *xsurface = xwayland_view->xwayland_surface;
	if (xsurface->surface == NULL) {
		*width_out = 0;
		*height_out = 0;
		return;
	}

	*width_out = xsurface->surface->current.width;
	*height_out = xsurface->surface->current.height;
}

static bool
is_primary(struct gf_view *view)
{
	struct gf_xwayland_view *xwayland_view = xwayland_view_from_view(view);
	struct wlr_xwayland_surface *parent = xwayland_view->xwayland_surface->parent;
	return parent == NULL;
}

static bool
is_transient_for(struct gf_view *child, struct gf_view *parent)
{
	if (parent->type != GAMEFRAME_XDG_SHELL_VIEW) {
		return false;
	}
	struct gf_xwayland_view *_child = xwayland_view_from_view(child);
	struct wlr_xwayland_surface *xwayland_surface = _child->xwayland_surface;
	struct gf_xwayland_view *_parent = xwayland_view_from_view(parent);
	struct wlr_xwayland
