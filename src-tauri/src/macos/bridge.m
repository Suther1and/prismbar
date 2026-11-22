#import <Cocoa/Cocoa.h>
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreFoundation/CoreFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <QuickLookThumbnailing/QuickLookThumbnailing.h>
#import <QuartzCore/QuartzCore.h>
#import <Carbon/Carbon.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef void (*TbMacKeyEventCallback)(
    int32_t event_kind,
    uint16_t key_code,
    uint64_t cg_flags,
    uint64_t timestamp_ms,
    void *ctx
);

typedef void (*TbMacVoidCallback)(void *ctx);

static inline void tb_run_on_main_sync(dispatch_block_t block) {
    if ([NSThread isMainThread]) {
        block();
    } else {
        dispatch_sync(dispatch_get_main_queue(), block);
    }
}

static NSString * const kTbBlurBackdropIdentifier = @"tb_mac_blur_backdrop";
static id g_resign_observer = nil;
static TbMacVoidCallback g_resign_cb = NULL;
static void *g_resign_ctx = NULL;

static void tb_clear_webview_backgrounds(NSView *view) {
    if (!view || [view isKindOfClass:[NSVisualEffectView class]]) {
        return;
    }
    if ([[view identifier] isEqualToString:kTbBlurBackdropIdentifier]) {
        return;
    }

    [view setWantsLayer:YES];
    CALayer *layer = [view layer];
    if (layer) {
        [layer setOpaque:NO];
        [layer setBackgroundColor:[[NSColor clearColor] CGColor]];
    }

    Class wk_class = NSClassFromString(@"WKWebView");
    NSString *cls_name = NSStringFromClass([view class]);
    if ((wk_class && [view isKindOfClass:wk_class]) ||
        (cls_name && [cls_name containsString:@"WKWebView"])) {
        @try {
            [view setValue:@NO forKey:@"drawsBackground"];
        } @catch (NSException *ex) {
            (void)ex;
        }
        SEL under_page_sel = NSSelectorFromString(@"setUnderPageBackgroundColor:");
        if ([view respondsToSelector:under_page_sel]) {
            @try {
                [view setValue:[NSColor clearColor] forKey:@"underPageBackgroundColor"];
            } @catch (NSException *ex) {
                (void)ex;
            }
        }
    }

    NSArray<NSView *> *children = [[view subviews] copy];
    for (NSView *child in children) {
        tb_clear_webview_backgrounds(child);
    }
}

static NSImage *tb_rounded_mask_image(CGFloat radius) {
    CGFloat edge = radius * 2.0 + 2.0;
    NSSize size = NSMakeSize(edge, edge);
    NSImage *mask = [NSImage imageWithSize:size
                                   flipped:NO
                            drawingHandler:^BOOL(NSRect dstRect) {
                                [[NSColor clearColor] setFill];
                                NSRectFill(dstRect);
                                [[NSColor blackColor] setFill];
                                NSBezierPath *path = [NSBezierPath bezierPathWithRoundedRect:dstRect
                                                                                     xRadius:radius
                                                                                     yRadius:radius];
                                [path fill];
                                return YES;
                            }];
    [mask setCapInsets:NSEdgeInsetsMake(radius, radius, radius, radius)];
    [mask setResizingMode:NSImageResizingModeStretch];
    return mask;
}

static NSVisualEffectView *tb_find_blur_view(NSView *content_view) {
    if (!content_view) {
        return nil;
    }
    for (NSView *sub in [content_view subviews]) {
        if ([sub isKindOfClass:[NSVisualEffectView class]] &&
            [[sub identifier] isEqualToString:kTbBlurBackdropIdentifier]) {
            return (NSVisualEffectView *)sub;
        }
    }
    return nil;
}

static void tb_ensure_visual_effect_view(NSWindow *win, bool blur_enabled) {
    if (!win) {
        return;
    }
    NSView *content = [win contentView];
    if (!content) {
        return;
    }
    NSRect frame = NSInsetRect([content bounds], 24.0, 24.0);
    if (frame.size.width <= 0.0 || frame.size.height <= 0.0) {
        frame = [content bounds];
    }
    if (frame.size.width <= 0.0 || frame.size.height <= 0.0) {
        return;
    }

    NSVisualEffectView *vev = tb_find_blur_view(content);
    if (!vev) {
        vev = [[NSVisualEffectView alloc] initWithFrame:frame];
        [vev setIdentifier:kTbBlurBackdropIdentifier];
        [vev setAutoresizingMask:(NSViewWidthSizable | NSViewHeightSizable)];
        [vev setMaterial:NSVisualEffectMaterialHUDWindow];
        [vev setBlendingMode:NSVisualEffectBlendingModeBehindWindow];
        [vev setState:NSVisualEffectStateActive];
        NSAppearance *dark = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
        if (dark) {
            [vev setAppearance:dark];
        }
        [vev setMaskImage:tb_rounded_mask_image(26.0)];
        [vev setWantsLayer:YES];
        CALayer *vev_layer = [vev layer];
        if (vev_layer) {
            [vev_layer setCornerRadius:26.0];
            [vev_layer setMasksToBounds:YES];
        }
        [content addSubview:vev positioned:NSWindowBelow relativeTo:nil];
    } else {
        [vev setFrame:frame];
    }

    [vev setMaterial:NSVisualEffectMaterialHUDWindow];
    [vev setBlendingMode:NSVisualEffectBlendingModeBehindWindow];
    [vev setState:NSVisualEffectStateActive];
    [vev setHidden:blur_enabled ? NO : YES];
}

void tb_mac_configure_start_window(void *ns_window_ptr) {
    if (!ns_window_ptr) {
        return;
    }
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (!win) {
                return;
            }
            [win setOpaque:NO];
            [win setBackgroundColor:[NSColor clearColor]];
            [win setHasShadow:NO];
            [win setLevel:NSFloatingWindowLevel];
            [win setHidesOnDeactivate:NO];
            [win setCollectionBehavior:(
                NSWindowCollectionBehaviorCanJoinAllSpaces |
                NSWindowCollectionBehaviorFullScreenAuxiliary |
                NSWindowCollectionBehaviorTransient |
                NSWindowCollectionBehaviorIgnoresCycle
            )];
            NSAppearance *dark = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
            if (dark) {
                [win setAppearance:dark];
            }
            [win setMovable:NO];
            tb_clear_webview_backgrounds([win contentView]);
            tb_ensure_visual_effect_view(win, true);
        }
    });
}

void tb_mac_set_blur_enabled(void *ns_window_ptr, bool enabled) {
    if (!ns_window_ptr) {
        return;
    }
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (!win) {
                return;
            }
            tb_clear_webview_backgrounds([win contentView]);
            tb_ensure_visual_effect_view(win, enabled);
        }
    });
}

void tb_mac_show_and_focus_window(void *ns_window_ptr, bool blur_enabled) {
    if (!ns_window_ptr) {
        return;
    }
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (!win) {
                return;
            }
            tb_clear_webview_backgrounds([win contentView]);
            tb_ensure_visual_effect_view(win, blur_enabled);
            [win setOpaque:NO];
            [win setBackgroundColor:[NSColor clearColor]];
            [win setHasShadow:NO];
            [[NSApplication sharedApplication] unhideWithoutActivation];
            [win setAlphaValue:1.0];
            [win setLevel:NSFloatingWindowLevel];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            [[NSApplication sharedApplication] activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
            [win makeKeyAndOrderFront:nil];
            [win orderFrontRegardless];
        }
    });
}

void tb_mac_hide_window(void *ns_window_ptr, bool yield_app_focus) {
    if (!ns_window_ptr) {
        return;
    }
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (win) {
                [win setAlphaValue:0.0];
                [win orderOut:nil];
            }
            if (yield_app_focus) {
                [[NSApplication sharedApplication] hide:nil];
            }
        }
    });
}

bool tb_mac_is_window_visible(void *ns_window_ptr) {
    if (!ns_window_ptr) {
        return false;
    }
    __block bool vis = false;
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (win) {
                vis = ([win isVisible] && [win alphaValue] > 0.01) ? true : false;
            }
        }
    });
    return vis;
}

void tb_mac_register_resign_active_observer(TbMacVoidCallback cb, void *ctx) {
    if (!cb) {
        return;
    }
    tb_run_on_main_sync(^{
        @autoreleasepool {
            g_resign_cb = cb;
            g_resign_ctx = ctx;
            NSNotificationCenter *nc = [NSNotificationCenter defaultCenter];
            if (g_resign_observer) {
                [nc removeObserver:g_resign_observer];
                g_resign_observer = nil;
            }
            g_resign_observer = [nc
                addObserverForName:NSApplicationDidResignActiveNotification
                            object:[NSApplication sharedApplication]
                             queue:nil
                        usingBlock:^(NSNotification * _Nonnull note) {
                            (void)note;
                            TbMacVoidCallback fn = g_resign_cb;
                            void *c = g_resign_ctx;
                            if (fn) {
                                fn(c);
                            }
                        }];
        }
    });
}

void tb_mac_position_start_window(
    void *ns_window_ptr,
    double width,
    double height,
    const char *position_mode
) {
    if (!ns_window_ptr) {
        return;
    }
    BOOL is_bottom = (position_mode != NULL && strcmp(position_mode, "bottom") == 0);
    double w = width > 0.0 ? width : 1188.0;
    double h = height > 0.0 ? height : 728.0;

    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSWindow *win = (__bridge NSWindow *)ns_window_ptr;
            if (!win) {
                return;
            }
            NSPoint mouse_loc = [NSEvent mouseLocation];
            NSScreen *target_screen = nil;
            for (NSScreen *scr in [NSScreen screens]) {
                if (NSPointInRect(mouse_loc, [scr frame])) {
                    target_screen = scr;
                    break;
                }
            }
            if (!target_screen) {
                target_screen = [NSScreen mainScreen];
            }
            if (!target_screen && [[NSScreen screens] count] > 0) {
                target_screen = [[NSScreen screens] firstObject];
            }
            if (!target_screen) {
                return;
            }

            NSRect vis = [target_screen visibleFrame];
            double x = vis.origin.x + (vis.size.width - w) * 0.5;
            double y = is_bottom
                ? (vis.origin.y + 14.0)
                : (vis.origin.y + (vis.size.height - h) * 0.5);

            if (x < vis.origin.x) {
                x = vis.origin.x;
            }
            if (y + h > NSMaxY(vis)) {
                y = NSMaxY(vis) - h;
            }
            if (y < vis.origin.y) {
                y = vis.origin.y;
            }

            [win setFrame:NSMakeRect(x, y, w, h) display:NO];
        }
    });
}

static uint8_t *tb_encode_nsimage_to_png(NSImage *image, int32_t size, size_t *out_len) {
    if (!image || size <= 0 || !out_len) {
        if (out_len) {
            *out_len = 0;
        }
        return NULL;
    }

    int32_t px = size > 512 ? 512 : size;
    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc]
        initWithBitmapDataPlanes:NULL
                      pixelsWide:px
                      pixelsHigh:px
                   bitsPerSample:8
                 samplesPerPixel:4
                        hasAlpha:YES
                        isPlanar:NO
                  colorSpaceName:NSCalibratedRGBColorSpace
                     bytesPerRow:0
                    bitsPerPixel:0];
    if (!rep) {
        *out_len = 0;
        return NULL;
    }
    [rep setSize:NSMakeSize((CGFloat)px, (CGFloat)px)];

    NSGraphicsContext *ctx = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
    if (!ctx) {
        *out_len = 0;
        return NULL;
    }

    [NSGraphicsContext saveGraphicsState];
    [NSGraphicsContext setCurrentContext:ctx];
    [[NSColor clearColor] setFill];
    NSRectFill(NSMakeRect(0.0, 0.0, (CGFloat)px, (CGFloat)px));
    [ctx setImageInterpolation:NSImageInterpolationHigh];

    NSSize img_size = [image size];
    NSRect dst_rect = NSMakeRect(0.0, 0.0, (CGFloat)px, (CGFloat)px);
    if (img_size.width > 0.0 && img_size.height > 0.0) {
        CGFloat scale = MIN((CGFloat)px / img_size.width, (CGFloat)px / img_size.height);
        CGFloat draw_w = img_size.width * scale;
        CGFloat draw_h = img_size.height * scale;
        dst_rect = NSMakeRect(
            ((CGFloat)px - draw_w) * 0.5,
            ((CGFloat)px - draw_h) * 0.5,
            draw_w,
            draw_h
        );
    }

    [image drawInRect:dst_rect
             fromRect:NSZeroRect
            operation:NSCompositingOperationSourceOver
             fraction:1.0];
    [ctx flushGraphics];
    [NSGraphicsContext restoreGraphicsState];

    NSData *png_data = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    if (!png_data || [png_data length] == 0) {
        *out_len = 0;
        return NULL;
    }

    size_t len = [png_data length];
    uint8_t *buf = (uint8_t *)malloc(len);
    if (!buf) {
        *out_len = 0;
        return NULL;
    }
    memcpy(buf, [png_data bytes], len);
    *out_len = len;
    return buf;
}

uint8_t *tb_mac_icon_png_for_path(const char *path_utf8, int32_t size, size_t *out_len) {
    if (out_len) {
        *out_len = 0;
    }
    if (!path_utf8 || !out_len) {
        return NULL;
    }
    @autoreleasepool {
        NSString *path = [NSString stringWithUTF8String:path_utf8];
        if (!path || [path length] == 0) {
            return NULL;
        }
        if (![[NSFileManager defaultManager] fileExistsAtPath:path]) {
            return NULL;
        }
        NSImage *icon = [[NSWorkspace sharedWorkspace] iconForFile:path];
        if (!icon) {
            return NULL;
        }
        int32_t target_size = size > 0 ? size : 64;
        return tb_encode_nsimage_to_png(icon, target_size, out_len);
    }
}

static BOOL tb_is_raster_image_extension(NSString *ext) {
    if (!ext || [ext length] == 0) {
        return NO;
    }
    NSString *low = [ext lowercaseString];
    return [low isEqualToString:@"png"] ||
           [low isEqualToString:@"jpg"] ||
           [low isEqualToString:@"jpeg"] ||
           [low isEqualToString:@"webp"] ||
           [low isEqualToString:@"gif"] ||
           [low isEqualToString:@"bmp"] ||
           [low isEqualToString:@"tiff"] ||
           [low isEqualToString:@"tif"] ||
           [low isEqualToString:@"heic"] ||
           [low isEqualToString:@"icns"] ||
           [low isEqualToString:@"ico"];
}

uint8_t *tb_mac_thumbnail_png_for_path(const char *path_utf8, int32_t size, size_t *out_len) {
    if (out_len) {
        *out_len = 0;
    }
    if (!path_utf8 || !out_len) {
        return NULL;
    }
    @autoreleasepool {
        NSString *path = [NSString stringWithUTF8String:path_utf8];
        if (!path || [path length] == 0) {
            return NULL;
        }
        if (![[NSFileManager defaultManager] fileExistsAtPath:path]) {
            return NULL;
        }
        int32_t target_size = size > 0 ? size : 64;

        if (tb_is_raster_image_extension([path pathExtension])) {
            NSImage *direct = [[NSImage alloc] initWithContentsOfFile:path];
            if (direct && [direct isValid]) {
                uint8_t *png = tb_encode_nsimage_to_png(direct, target_size, out_len);
                if (png) {
                    return png;
                }
            }
        }

        NSURL *url = [NSURL fileURLWithPath:path];
        if (url) {
            QLThumbnailGenerationRequest *req = [[QLThumbnailGenerationRequest alloc]
                initWithFileAtURL:url
                             size:CGSizeMake((CGFloat)target_size, (CGFloat)target_size)
                            scale:1.0
              representationTypes:QLThumbnailGenerationRequestRepresentationTypeAll];
            dispatch_semaphore_t sem = dispatch_semaphore_create(0);
            __block NSImage *ql_image = nil;
            [[QLThumbnailGenerator sharedGenerator]
                generateBestRepresentationForRequest:req
                                   completionHandler:^(
                                       QLThumbnailRepresentation * _Nullable thumbnail,
                                       NSError * _Nullable error
                                   ) {
                                       (void)error;
                                       if (thumbnail) {
                                           CGImageRef cg = [thumbnail CGImage];
                                           if (cg) {
                                               ql_image = [[NSImage alloc]
                                                   initWithCGImage:cg
                                                              size:NSMakeSize(
                                                                  (CGFloat)CGImageGetWidth(cg),
                                                                  (CGFloat)CGImageGetHeight(cg)
                                                              )];
                                           }
                                       }
                                       dispatch_semaphore_signal(sem);
                                   }];
            dispatch_time_t timeout = dispatch_time(
                DISPATCH_TIME_NOW,
                (int64_t)(250 * NSEC_PER_MSEC)
            );
            if (dispatch_semaphore_wait(sem, timeout) == 0 && ql_image != nil) {
                uint8_t *png = tb_encode_nsimage_to_png(ql_image, target_size, out_len);
                if (png) {
                    return png;
                }
            }
        }

        (void)target_size;
        return NULL;
    }
}

int64_t tb_mac_clipboard_change_count(void) {
    @autoreleasepool {
        return (int64_t)[[NSPasteboard generalPasteboard] changeCount];
    }
}

char *tb_mac_clipboard_get_text(void) {
    @autoreleasepool {
        NSString *str = [[NSPasteboard generalPasteboard] stringForType:NSPasteboardTypeString];
        if (!str) {
            return NULL;
        }
        const char *utf8 = [str UTF8String];
        return utf8 ? strdup(utf8) : NULL;
    }
}

bool tb_mac_clipboard_set_text(const char *text_utf8) {
    if (!text_utf8) {
        return false;
    }
    @autoreleasepool {
        NSString *str = [NSString stringWithUTF8String:text_utf8];
        if (!str) {
            return false;
        }
        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        [pb clearContents];
        return [pb setString:str forType:NSPasteboardTypeString] ? true : false;
    }
}

bool tb_mac_clipboard_set_files(const char *const *paths_utf8, size_t count) {
    if (!paths_utf8 || count == 0) {
        return false;
    }
    @autoreleasepool {
        NSMutableArray<NSURL *> *urls = [NSMutableArray arrayWithCapacity:count];
        for (size_t i = 0; i < count; i++) {
            if (!paths_utf8[i]) {
                continue;
            }
            NSString *p = [NSString stringWithUTF8String:paths_utf8[i]];
            if (p && [p length] > 0) {
                NSURL *u = [NSURL fileURLWithPath:p];
                if (u) {
                    [urls addObject:u];
                }
            }
        }
        if ([urls count] == 0) {
            return false;
        }
        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        [pb clearContents];
        return [pb writeObjects:urls] ? true : false;
    }
}

int32_t tb_mac_frontmost_app_pid(void) {
    @autoreleasepool {
        NSRunningApplication *app = [[NSWorkspace sharedWorkspace] frontmostApplication];
        return app ? (int32_t)[app processIdentifier] : 0;
    }
}

bool tb_mac_activate_pid(int32_t pid) {
    if (pid <= 0) {
        return false;
    }
    __block BOOL ok = NO;
    tb_run_on_main_sync(^{
        @autoreleasepool {
            NSRunningApplication *app =
                [NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)pid];
            if (app) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
                ok = [app activateWithOptions:NSApplicationActivateIgnoringOtherApps];
#pragma clang diagnostic pop
            }
        }
    });
    return ok ? true : false;
}

bool tb_mac_is_accessibility_trusted(bool prompt) {
    if (!prompt) {
        return AXIsProcessTrusted() ? true : false;
    }
    @autoreleasepool {
        NSDictionary *opts = @{ (__bridge NSString *)kAXTrustedCheckOptionPrompt : @YES };
        return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)opts) ? true : false;
    }
}

void tb_mac_open_accessibility_settings(void) {
    @autoreleasepool {
        NSURL *url = [NSURL URLWithString:
            @"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        ];
        if (url) {
            [[NSWorkspace sharedWorkspace] openURL:url];
        }
    }
}

static pthread_mutex_t g_tap_lock = PTHREAD_MUTEX_INITIALIZER;
static bool g_tap_running = false;
static CFMachPortRef g_tap_port = NULL;
static TbMacKeyEventCallback g_tap_cb = NULL;
static void *g_tap_ctx = NULL;

static CGEventRef tb_event_tap_callback(
    CGEventTapProxy proxy,
    CGEventType type,
    CGEventRef event,
    void *userInfo
) {
    (void)proxy;
    (void)userInfo;

    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (g_tap_port != NULL) {
            CGEventTapEnable(g_tap_port, true);
        }
        return event;
    }

    if (IsSecureEventInputEnabled()) {
        TbMacKeyEventCallback cb = g_tap_cb;
        void *ctx = g_tap_ctx;
        if (cb != NULL) {
            uint64_t now_ms = (uint64_t)(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) / 1000000ULL);
            cb(4, 0, 0, now_ms, ctx);
        }
        return event;
    }

    int32_t kind = 0;
    if (type == kCGEventFlagsChanged) {
        kind = 1;
    } else if (type == kCGEventKeyDown || type == kCGEventKeyUp) {
        kind = 2;
    } else if (
        type == kCGEventLeftMouseDown ||
        type == kCGEventRightMouseDown ||
        type == kCGEventOtherMouseDown ||
        type == kCGEventLeftMouseDragged ||
        type == kCGEventRightMouseDragged ||
        type == kCGEventScrollWheel
    ) {
        kind = 3;
    }

    if (kind != 0 && event != NULL) {
        TbMacKeyEventCallback cb = g_tap_cb;
        void *ctx = g_tap_ctx;
        if (cb != NULL) {
            uint16_t key_code = 0;
            if (kind == 1 || kind == 2) {
                int64_t kc = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
                if (kc >= 0 && kc <= UINT16_MAX) {
                    key_code = (uint16_t)kc;
                }
            }
            uint64_t flags = (uint64_t)CGEventGetFlags(event);
            uint64_t now_ms = (uint64_t)(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) / 1000000ULL);
            cb(kind, key_code, flags, now_ms, ctx);
        }
    }

    return event;
}

bool tb_mac_start_command_tap(TbMacKeyEventCallback cb, void *ctx) {
    if (!cb) {
        return false;
    }

    pthread_mutex_lock(&g_tap_lock);
    if (g_tap_running) {
        g_tap_cb = cb;
        g_tap_ctx = ctx;
        pthread_mutex_unlock(&g_tap_lock);
        return true;
    }

    g_tap_cb = cb;
    g_tap_ctx = ctx;
    g_tap_running = true;

    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    __block bool started = false;

    [NSThread detachNewThreadWithBlock:^{
        @autoreleasepool {
            CGEventMask mask =
                CGEventMaskBit(kCGEventFlagsChanged) |
                CGEventMaskBit(kCGEventKeyDown) |
                CGEventMaskBit(kCGEventKeyUp) |
                CGEventMaskBit(kCGEventLeftMouseDown) |
                CGEventMaskBit(kCGEventRightMouseDown) |
                CGEventMaskBit(kCGEventOtherMouseDown) |
                CGEventMaskBit(kCGEventLeftMouseDragged) |
                CGEventMaskBit(kCGEventRightMouseDragged) |
                CGEventMaskBit(kCGEventScrollWheel);

            CFMachPortRef tap = CGEventTapCreate(
                kCGSessionEventTap,
                kCGHeadInsertEventTap,
                kCGEventTapOptionDefault,
                mask,
                tb_event_tap_callback,
                NULL
            );

            if (!tap) {
                started = false;
                dispatch_semaphore_signal(sem);
                return;
            }

            CFRunLoopSourceRef src = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0);
            if (!src) {
                CFRelease(tap);
                started = false;
                dispatch_semaphore_signal(sem);
                return;
            }

            g_tap_port = tap;
            CFRunLoopRef rl = CFRunLoopGetCurrent();
            CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);

            started = true;
            dispatch_semaphore_signal(sem);

            CFRunLoopRun();

            CFRunLoopRemoveSource(rl, src, kCFRunLoopCommonModes);
            CFRelease(src);
            CFRelease(tap);
            g_tap_port = NULL;
        }
    }];

    dispatch_time_t timeout = dispatch_time(DISPATCH_TIME_NOW, (int64_t)(2000 * NSEC_PER_MSEC));
    long waited = dispatch_semaphore_wait(sem, timeout);
    bool ok = (waited == 0) && started;
    if (!ok) {
        g_tap_running = false;
    }
    pthread_mutex_unlock(&g_tap_lock);
    return ok;
}

void tb_mac_send_cmd_v(void) {
    CGEventSourceRef src = CGEventSourceCreate(kCGEventSourceStateHIDSystemState);
    CGEventRef down = CGEventCreateKeyboardEvent(src, (CGKeyCode)9, true);
    CGEventRef up = CGEventCreateKeyboardEvent(src, (CGKeyCode)9, false);
    if (down) {
        CGEventSetFlags(down, kCGEventFlagMaskCommand);
        CGEventPost(kCGHIDEventTap, down);
        CFRelease(down);
    }
    if (up) {
        CGEventSetFlags(up, kCGEventFlagMaskCommand);
        CGEventPost(kCGHIDEventTap, up);
        CFRelease(up);
    }
    if (src) {
        CFRelease(src);
    }
}

bool tb_mac_trash_item(const char *path_utf8, char *err_buf, size_t err_buf_len) {
    if (err_buf && err_buf_len > 0) {
        err_buf[0] = '\0';
    }
    if (!path_utf8) {
        if (err_buf && err_buf_len > 0) {
            snprintf(err_buf, err_buf_len, "Invalid path");
        }
        return false;
    }
    @autoreleasepool {
        NSString *path = [NSString stringWithUTF8String:path_utf8];
        if (!path || [path length] == 0) {
            if (err_buf && err_buf_len > 0) {
                snprintf(err_buf, err_buf_len, "Empty path");
            }
            return false;
        }
        NSURL *url = [NSURL fileURLWithPath:path];
        NSError *err = nil;
        BOOL ok = [[NSFileManager defaultManager] trashItemAtURL:url resultingItemURL:nil error:&err];
        if (!ok && err && err_buf && err_buf_len > 0) {
            const char *msg = [[err localizedDescription] UTF8String];
            if (msg) {
                snprintf(err_buf, err_buf_len, "%s", msg);
            }
        }
        return ok ? true : false;
    }
}

void tb_mac_free(void *ptr) {
    free(ptr);
}
