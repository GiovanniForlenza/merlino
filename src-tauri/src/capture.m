// capture.m — ScreenCaptureKit helper
// sck_init_capture: chiamare una volta al pin (lento, ~1-3s)
// sck_capture_window: chiamare ogni frame (veloce, ~50-100ms)
// sck_stop_capture: chiamare al de-pin

#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

// Cache windowID → SCContentFilter
static NSMutableDictionary *gFilters = nil;

// Inizializza il filtro per la finestra (chiamata lenta, una sola volta).
// Restituisce 1 se OK, 0 se fallisce.
int sck_init_capture(uint32_t windowID) {
    if (!gFilters) {
        gFilters = [[NSMutableDictionary alloc] init];
    }

    __block BOOL ok = NO;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);

    [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *content, NSError *error) {
        if (!content) { dispatch_semaphore_signal(sem); return; }

        SCWindow *target = nil;
        for (SCWindow *w in content.windows) {
            if (w.windowID == windowID) { target = w; break; }
        }
        if (target) {
            SCContentFilter *f = [[SCContentFilter alloc] initWithDesktopIndependentWindow:target];
            gFilters[@(windowID)] = f;
            ok = YES;
        }
        dispatch_semaphore_signal(sem);
    }];

    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 5LL * NSEC_PER_SEC));
    return ok ? 1 : 0;
}

// Cattura un frame usando il filtro cachato.
// Restituisce un CGImageRef (retained) o NULL.
CGImageRef sck_capture_window(uint32_t windowID) {
    SCContentFilter *filter = gFilters[@(windowID)];
    if (!filter) return NULL;

    __block CGImageRef result = NULL;
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);

    SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
    // 1x resolution (non Retina) per ridurre il carico IPC
    config.width  = (size_t)(filter.contentRect.size.width);
    config.height = (size_t)(filter.contentRect.size.height);

    [SCScreenshotManager captureImageWithFilter:filter
                                  configuration:config
                              completionHandler:^(CGImageRef image, NSError *err) {
        if (image) { CGImageRetain(image); result = image; }
        dispatch_semaphore_signal(sem);
    }];

    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 2LL * NSEC_PER_SEC));
    return result;
}

// Rimuove il filtro cachato per la finestra.
void sck_stop_capture(uint32_t windowID) {
    [gFilters removeObjectForKey:@(windowID)];
}
