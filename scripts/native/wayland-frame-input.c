/* Native Linux test driver. Build with a scanner-generated fake-input client
 * header/code from KDE's plasma-wayland-protocols fake-input.xml (LGPL-2.1+).
 * This is test automation only; Rodtty itself uses winit's native input serial. */
#include <wayland-client.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include "fake-input-client.h"
static struct org_kde_kwin_fake_input *input;
static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data;
    if (!strcmp(interface, "org_kde_kwin_fake_input"))
        input = wl_registry_bind(registry, name, &org_kde_kwin_fake_input_interface,
                                 version < 5 ? version : 5);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener listener = {global, removed};
int main(void) {
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) { perror("Wayland connect"); return 1; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    wl_display_roundtrip(display);
    if (!input) { fputs("KWin fake-input testing protocol unavailable\n", stderr); return 2; }
    org_kde_kwin_fake_input_authenticate(input, "Rodtty native frame check", "Exercise custom frame with actual pointer input");
    wl_display_roundtrip(display);
    char op[16]; double x, y;
    while (scanf("%15s %lf %lf", op, &x, &y) == 3) {
        if (!strcmp(op, "move")) org_kde_kwin_fake_input_pointer_motion_absolute(input, wl_fixed_from_double(x), wl_fixed_from_double(y));
        else if (!strcmp(op, "key")) org_kde_kwin_fake_input_keyboard_key(input, (uint32_t)x, (uint32_t)y);
        else if (!strcmp(op, "button")) org_kde_kwin_fake_input_button(input, (uint32_t)x, (uint32_t)y);
        wl_display_roundtrip(display);
        puts("ok"); fflush(stdout);
    }
    wl_display_disconnect(display);
    return 0;
}
