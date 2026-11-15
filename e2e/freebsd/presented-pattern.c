// Read only colour statistics for the synthetic pattern, never save desktop images.
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <stdlib.h>

static unsigned long channel(unsigned long pixel, unsigned long mask) {
    if (!mask) return 0;
    while (!(mask & 1)) { mask >>= 1; pixel >>= 1; }
    return (pixel & mask) * 255 / mask;
}

int main(int argc, char **argv) {
    if (argc != 5) return 2;
    Display *display = XOpenDisplay(NULL);
    if (!display) return 3;
    XWindowAttributes root;
    if (!XGetWindowAttributes(display, DefaultRootWindow(display), &root)) {
        XCloseDisplay(display);
        return 4;
    }
    int x = atoi(argv[1]), y = atoi(argv[2]);
    int width = atoi(argv[3]), height = atoi(argv[4]);
    if (x < 0) x = 0;
    if (y < 0) y = 0;
    if (x >= root.width || y >= root.height || width <= 0 || height <= 0) {
        XCloseDisplay(display);
        return 5;
    }
    if (width > root.width - x) width = root.width - x;
    if (height > root.height - y) height = root.height - y;
    XImage *image = XGetImage(display, DefaultRootWindow(display), x, y,
        (unsigned)width, (unsigned)height, AllPlanes, ZPixmap);
    if (!image) { XCloseDisplay(display); return 6; }
    unsigned long sum_r = 0, sum_g = 0, sum_b = 0, matched = 0;
    for (int py = 0; py < image->height; py++) {
        for (int px = 0; px < image->width; px++) {
            unsigned long pixel = XGetPixel(image, px, py);
            unsigned long r = channel(pixel, image->red_mask);
            unsigned long g = channel(pixel, image->green_mask);
            unsigned long b = channel(pixel, image->blue_mask);
            if (r <= 27 && b >= 225 && g >= 40 && g <= 215) {
                matched++; sum_r += r; sum_g += g; sum_b += b;
            }
        }
    }
    if (matched) printf("{\"matched\":%lu,\"pixel\":[%lu,%lu,%lu]}\n", matched, sum_r / matched, sum_g / matched, sum_b / matched);
    else puts("{\"matched\":0,\"pixel\":[0,0,0]}");
    XDestroyImage(image);
    XCloseDisplay(display);
    return 0;
}
