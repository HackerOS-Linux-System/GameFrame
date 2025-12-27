#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <limits.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/backend/headless.h>
#include <wlr/backend/multi.h>
#include <wlr/backend/session.h>
#include <wlr/config.h>
#include <wlr/render/gles2.h>
#include <wlr/render/wlr_renderer.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_gamma_control_v1.h>
#include <wlr/types/wlr_idle.h>
#include <wlr/types/wlr_idle_inhibit_v1.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_output_manager_v1.h>
#include <wlr/types/wlr_presentation.h>
#include <wlr/types/wlr_primary_selection.h>
#include <wlr/types/wlr_relative_pointer_v1.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_screencopy_v1.h>
#include <wlr/types/wlr_server_decoration.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_tablet_v2.h>
#include <wlr/types/wlr_virtual_keyboard_v1.h>
#include <wlr/types/wlr_virtual_pointer_v1.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_output_v1.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/types/wlr_xdg_decoration_v1.h>
#include <wlr/util/log.h>
#include <wlr/util/region.h>

#include "idle_inhibit_v1.h"
#include "output.h"
#include "seat.h"
#include "server.h"
#include "view.h"
#include "xdg_shell.h"
#if GAMEFRAME_HAS_XWAYLAND
#include "xwayland.h"
#endif

void
server_terminate(struct gf_server *server)
{
	// Workaround for https://gitlab.freedesktop.org/wayland/wayland/-/merge_requests/421
	if (server->terminated) {
		return;
	}

	wl_display_terminate(server->wl_display);
}

static void
handle_display_destroy(struct wl_listener *listener, void *data)
{
	struct gf_server *server = wl_container_of(listener, server, display_destroy);
	server->terminated = true;
}

static int
sigchld_handler(int fd, uint32_t mask, void *data)
{
	struct gf_server *server = data;

	/* Close Gameframe's read pipe. */
	close(fd);

	if (mask & WL_EVENT_HANGUP) {
		wlr_log(WLR_DEBUG, "Child process closed normally");
	} else if (mask & WL_EVENT_ERROR) {
		wlr_log(WLR_DEBUG, "Connection closed by server");
	}

	server->return_app_code = true;
	server_terminate(server);
	return 0;
}

static bool
set_cloexec(int fd)
{
	int flags = fcntl(fd, F_GETFD);

	if (flags == -1) {
		wlr_log(WLR_ERROR, "Unable to set the CLOEXEC flag: fnctl failed");
		return false;
	}

	flags = flags | FD_CLOEXEC;
	if (fcntl(fd, F_SETFD, flags) == -1) {
		wlr_log(WLR_ERROR, "Unable to set the CLOEXEC flag: fnctl failed");
		return false;
	}

	return true;
}

static bool
spawn_primary_client(struct gf_server *server, char *argv[], pid_t *pid_out, struct wl_event_source **sigchld_source)
{
	int fd[2];
	if (pipe(fd) != 0) {
		wlr_log(WLR_ERROR, "Unable to create pipe");
		return false;
	}

	pid_t pid = fork();
	if (pid == 0) {
		sigset_t set;
		sigemptyset(&set);
		sigprocmask(SIG_SETMASK, &set, NULL);
		/* Close read, we only need write in the primary client process. */
		close(fd[0]);
		execvp(argv[0], argv);
		/* execvp() returns only on failure */
		wlr_log_errno(WLR_ERROR, "Failed to spawn client");
		_exit(1);
	} else if (pid == -1) {
		wlr_log_errno(WLR_ERROR, "Unable to fork");
		return false;
	}

	/* Set this early so that if we fail, the client process will be cleaned up properly. */
	*pid_out = pid;

	if (!set_cloexec(fd[0]) || !set_cloexec(fd[1])) {
		return false;
	}

	/* Close write, we only need read in Gameframe. */
	close(fd[1]);

	struct wl_event_loop *event_loop = wl_display_get_event_loop(server->wl_display);
	uint32_t mask = WL_EVENT_HANGUP | WL_EVENT_ERROR;
	*sigchld_source = wl_event_loop_add_fd(event_loop, fd[0], mask, sigchld_handler, server);

	wlr_log(WLR_DEBUG, "Child process created with pid %d", pid);
	return true;
}

static int
cleanup_primary_client(pid_t pid)
{
	int status;

	waitpid(pid, &status, 0);

	if (WIFEXITED(status)) {
		wlr_log(WLR_DEBUG, "Child exited normally with exit status %d", WEXITSTATUS(status));
		return WEXITSTATUS(status);
	} else if (WIFSIGNALED(status)) {
		/* Mimic Bash and other shells for the exit status */
		wlr_log(WLR_DEBUG, "Child was terminated by a signal (%d)", WTERMSIG(status));
		return 128 + WTERMSIG(status);
	}

	return 0;
}

static bool
drop_permissions(void)
{
	if (getuid() == 0 || getgid() == 0) {
		wlr_log(WLR_INFO, "Running as root user, this is dangerous");
		return true;
	}
	if (getuid() != geteuid() || getgid() != getegid()) {
		wlr_log(WLR_INFO, "setuid/setgid bit detected, dropping permissions");
		// Set the gid and uid in the correct order.
		if (setgid(getgid()) != 0 || setuid(getuid()) != 0) {
			wlr_log(WLR_ERROR, "Unable to drop root, refusing to start");
			return false;
		}
	}

	if (setgid(0) != -1 || setuid(0) != -1) {
		wlr_log(WLR_ERROR, "Unable to drop root (we shouldn't be able to restore it after setuid), refusing to start");
		return false;
	}

	return true;
}

static int
handle_signal(int signal, void *data)
{
	struct gf_server *server = data;

	switch (signal) {
	case SIGINT:
		/* Fallthrough */
	case SIGTERM:
		server_terminate(server);
		return 0;
	default:
		return 0;
	}
}

static void
usage(FILE *file, const char *gameframe)
{
	fprintf(file,
		"Usage: %s [OPTIONS] [--] [APPLICATION...]\n"
		"\n"
		" -d\t Don't draw client side decorations, when possible\n"
		" -D\t Enable debug logging\n"
		" -h\t Display this help message\n"
		" -m extend Extend the display across all connected outputs (default)\n"
		" -m last Use only the last connected output\n"
		" -s\t Allow VT switching\n"
		" -v\t Show the version number and exit\n"
		" -W <width>\t Set the resolution used by gameframe (output resolution)\n"
		" -H <height>\t Set the resolution used by gameframe (output resolution)\n"
		" -w <width>\t Set the resolution used by the game (inner resolution)\n"
		" -h <height>\t Set the resolution used by the game (inner resolution)\n"
		" -r <fps>\t Set frame-rate limit for the game when focused\n"
		" -o <fps>\t Set frame-rate limit for the game when unfocused\n"
		" -F fsr\t Use AMD FSR upscaling (parsed but uses basic scaling on older GPUs)\n"
		" -F nis\t Use NVIDIA NIS upscaling (parsed but uses basic scaling on older GPUs)\n"
		" -S integer\t Use integer scaling\n"
		" -S stretch\t Use stretch scaling\n"
		" -b\t Create a border-less window\n"
		" -f\t Create a full-screen window\n"
		" --reshade-effect [path]\t Specify a Reshade effect file (parsed but not implemented)\n"
		" --reshade-technique-idx [idx]\t Specify Reshade technique index (parsed but not implemented)\n"
		"\n"
		" Use -- when you want to pass arguments to APPLICATION\n",
		gameframe);
}

static bool
parse_args(struct gf_server *server, int argc, char *argv[])
{
	int c;
	char *upscale_method = NULL;
	char *scaling_method = NULL;
	char *reshade_path = NULL;
	int reshade_idx = -1;

	static struct option long_options[] = {
		{"reshade-effect", required_argument, 0, 0},
		{"reshade-technique-idx", required_argument, 0, 0},
		{0, 0, 0, 0}
	};

	int option_index = 0;
	while ((c = getopt_long(argc, argv, "dDhm:svo:r:w:h:W:H:F:S:bf", long_options, &option_index)) != -1) {
		switch (c) {
		case 0:
			if (strcmp(long_options[option_index].name, "reshade-effect") == 0) {
				reshade_path = optarg;
				wlr_log(WLR_INFO, "Reshade effect parsed but not implemented on older GPUs");
			} else if (strcmp(long_options[option_index].name, "reshade-technique-idx") == 0) {
				reshade_idx = atoi(optarg);
				wlr_log(WLR_INFO, "Reshade index parsed but not implemented on older GPUs");
			}
			break;
		case 'd':
			server->xdg_decoration = true;
			break;
		case 'D':
			server->log_level = WLR_DEBUG;
			break;
		case 'h':
			usage(stdout, argv[0]);
			return false;
		case 'm':
			if (strcmp(optarg, "last") == 0) {
				server->output_mode = GAMEFRAME_MULTI_OUTPUT_MODE_LAST;
			} else if (strcmp(optarg, "extend") == 0) {
				server->output_mode = GAMEFRAME_MULTI_OUTPUT_MODE_EXTEND;
			}
			break;
		case 's':
			server->allow_vt_switch = true;
			break;
		case 'v':
			fprintf(stdout, "Gameframe version " GAMEFRAME_VERSION "\n");
			exit(0);
		case 'W':
			server->nested_width = atoi(optarg);
			break;
		case 'H':
			server->nested_height = atoi(optarg);
			break;
		case 'w':
			server->game_width = atoi(optarg);
			break;
		case 'h':
			server->game_height = atoi(optarg);
			break;
		case 'r':
			server->fps_focused = atoi(optarg);
			break;
		case 'o':
			server->fps_unfocused = atoi(optarg);
			break;
		case 'F':
			upscale_method = optarg;
			wlr_log(WLR_INFO, "Upscaling method %s parsed, using basic scaling on older GPUs", optarg);
			break;
		case 'S':
			scaling_method = optarg;
			wlr_log(WLR_INFO, "Scaling method %s parsed", optarg);
			break;
		case 'b':
			server->borderless = true;
			break;
		case 'f':
			server->fullscreen = true;
			break;
		default:
			usage(stderr, argv[0]);
			return false;
		}
	}

	return true;
}

int
main(int argc, char *argv[])
{
	struct gf_server server = {.log_level = WLR_INFO};
	struct wl_event_source *sigchld_source = NULL;
	pid_t pid = 0;
	int ret = 0, app_ret = 0;

#ifdef DEBUG
	server.log_level = WLR_DEBUG;
#endif

	server.nested_width = 1280;
	server.nested_height = 720;
	server.game_width = 1280;
	server.game_height = 720;
	server.fps_focused = 0; // Unlimited
	server.fps_unfocused = 0;
	server.borderless = false;
	server.fullscreen = false;

	if (!parse_args(&server, argc, argv)) {
		return 1;
	}

	wlr_log_init(server.log_level, NULL);

	/* Wayland requires XDG_RUNTIME_DIR to be set. */
	if (!getenv("XDG_RUNTIME_DIR")) {
		wlr_log(WLR_ERROR, "XDG_RUNTIME_DIR is not set in the environment");
		return 1;
	}

	server.wl_display = wl_display_create();
	if (!server.wl_display) {
		wlr_log(WLR_ERROR, "Cannot allocate a Wayland display");
		return 1;
	}

	server.display_destroy.notify = handle_display_destroy;
	wl_display_add_destroy_listener(server.wl_display, &server.display_destroy);

	struct wl_event_loop *event_loop = wl_display_get_event_loop(server.wl_display);
	struct wl_event_source *sigint_source = wl_event_loop_add_signal(event_loop, SIGINT, handle_signal, &server);
	struct wl_event_source *sigterm_source = wl_event_loop_add_signal(event_loop, SIGTERM, handle_signal, &server);

	server.backend = wlr_backend_autocreate(event_loop, &server.session);
	if (!server.backend) {
		wlr_log(WLR_ERROR, "Unable to create the wlroots backend");
		ret = 1;
		goto end;
	}

	if (!drop_permissions()) {
		ret = 1;
		goto end;
	}

	server.renderer = wlr_gles2_renderer_create(server.backend);
	if (!server.renderer) {
		wlr_log(WLR_ERROR, "Unable to create GLES2 renderer for older GPUs, falling back");
		server.renderer = wlr_renderer_autocreate(server.backend);
		if (!server.renderer) {
			wlr_log(WLR_ERROR, "Unable to create the wlroots renderer");
			ret = 1;
			goto end;
		}
	}

	server.allocator = wlr_allocator_autocreate(server.backend, server.renderer);
	if (!server.allocator) {
		wlr_log(WLR_ERROR, "Unable to create the wlroots allocator");
		ret = 1;
		goto end;
	}

	wlr_renderer_init_wl_display(server.renderer, server.wl_display);

	wl_list_init(&server.views);
	wl_list_init(&server.outputs);

	server.output_layout = wlr_output_layout_create(server.wl_display);
	if (!server.output_layout) {
		wlr_log(WLR_ERROR, "Unable to create output layout");
		ret = 1;
		goto end;
	}
	server.output_layout_change.notify = handle_output_layout_change;
	wl_signal_add(&server.output_layout->events.change, &server.output_layout_change);

	server.scene = wlr_scene_create();
	if (!server.scene) {
		wlr_log(WLR_ERROR, "Unable to create scene");
		ret = 1;
		goto end;
	}

	server.scene_output_layout = wlr_scene_attach_output_layout(server.scene, server.output_layout);

	struct wlr_compositor *compositor = wlr_compositor_create(server.wl_display, 6, server.renderer);
	if (!compositor) {
		wlr_log(WLR_ERROR, "Unable to create the wlroots compositor");
		ret = 1;
		goto end;
	}

	if (!wlr_subcompositor_create(server.wl_display)) {
		wlr_log(WLR_ERROR, "Unable to create the wlroots subcompositor");
		ret = 1;
		goto end;
	}

	if (!wlr_data_device_manager_create(server.wl_display)) {
		wlr_log(WLR_ERROR, "Unable to create the data device manager");
		ret = 1;
		goto end;
	}

	if (!wlr_primary_selection_v1_device_manager_create(server.wl_display)) {
		wlr_log(WLR_ERROR, "Unable to create primary selection device manager");
		ret = 1;
		goto end;
	}

	/* Configure a listener to be notified when new outputs are
	 * available on the backend. We use this only to detect the
	 * first output and ignore subsequent outputs. */
	server.new_output.notify = handle_new_output;
	wl_signal_add(&server.backend->events.new_output, &server.new_output);

	server.seat = seat_create(&server, server.backend);
	if (!server.seat) {
		wlr_log(WLR_ERROR, "Unable to create the seat");
		ret = 1;
		goto end;
	}

	server.idle = wlr_idle_notifier_v1_create(server.wl_display);

	server.idle_inhibit_v1 = wlr_idle_inhibit_v1_create(server.wl_display);
	server.new_idle_inhibitor_v1.notify = handle_idle_inhibitor_v1_new;
	wl_signal_add(&server.idle_inhibit_v1->events.new_inhibitor, &server.new_idle_inhibitor_v1);

	server.xdg_shell = wlr_xdg_shell_create(server.wl_display, 6);
	server.new_xdg_toplevel.notify = handle_new_xdg_toplevel;
	wl_signal_add(&server.xdg_shell->events.new_toplevel, &server.new_xdg_toplevel);
	server.new_xdg_popup.notify = handle_new_xdg_popup;
	wl_signal_add(&server.xdg_shell->events.new_popup, &server.new_xdg_popup);

	server.xdg_decoration_manager = wlr_xdg_decoration_manager_v1_create(server.wl_display);
	server.new_xdg_decoration.notify = handle_xdg_toplevel_decoration;
	wl_signal_add(&server.xdg_decoration_manager->events.new_toplevel_decoration, &server.new_xdg_decoration);

#if GAMEFRAME_HAS_XWAYLAND
	server.xwayland = wlr_xwayland_create(server.wl_display, compositor, true);
	server.new_xwayland_surface.notify = handle_new_xwayland_surface;
	wl_signal_add(&server.xwayland->events.new_surface, &server.new_xwayland_surface);
	setenv("DISPLAY", server.xwayland->display_name, true);
#endif

	server.output_manager_v1 = wlr_output_manager_v1_create(server.wl_display);
	server.output_manager_apply.notify = handle_output_manager_apply;
	wl_signal_add(&server.output_manager_v1->events.apply, &server.output_manager_apply);
	server.output_manager_test.notify = handle_output_manager_test;
	wl_signal_add(&server.output_manager_v1->events.test, &server.output_manager_test);

	server.relative_pointer_manager = wlr_relative_pointer_manager_v1_create(server.wl_display);

	server.foreign_toplevel_manager = wlr_foreign_toplevel_manager_v1_create(server.wl_display);

	const char *socket = wl_display_add_socket_auto(server.wl_display);
	if (!socket) {
		wlr_backend_destroy(server.backend);
		return 1;
	}
	setenv("WAYLAND_DISPLAY", socket, true);
	wlr_log(WLR_INFO, "Running Wayland compositor on WAYLAND_DISPLAY=%s", socket);

	if (!wlr_backend_start(server.backend)) {
		wlr_backend_destroy(server.backend);
		wl_display_destroy(server.wl_display);
		return 1;
	}

	if (optind < argc) {
		if (!spawn_primary_client(&server, &argv[optind], &pid, &sigchld_source)) {
			ret = 1;
			goto end;
		}
	} else {
		wlr_log(WLR_ERROR, "No application specified, exiting");
		ret = 1;
		goto end;
	}

	wl_display_run(server.wl_display);

	if (server.return_app_code) {
		app_ret = cleanup_primary_client(pid);
	}

end:

wl_display_destroy_clients(server.wl_display);

wlr_scene_output_layout_destroy(server.scene_output_layout);

wlr_scene_destroy(server.scene);

wlr_output_layout_destroy(server.output_layout);

wlr_allocator_destroy(server.allocator);

wlr_renderer_destroy(server.renderer);

wlr_backend_destroy(server.backend);

wl_display_destroy(server.wl_display);

wl_event_source_remove(sigint_source);

wl_event_source_remove(sigterm_source);

if (sigchld_source) {
	wl_event_source_remove(sigchld_source);
}

wlr_log(WLR_INFO, "Exiting");

return ret ? ret : app_ret;
}
