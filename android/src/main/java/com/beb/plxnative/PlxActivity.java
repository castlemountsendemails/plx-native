package com.beb.plxnative;

import android.content.Context;
import android.content.Intent;
import android.graphics.PixelFormat;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaFormat;
import android.os.Bundle;
import android.os.SystemClock;
import android.util.Log;
import android.view.KeyEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.ViewGroup;
import android.widget.RelativeLayout;

import java.util.ArrayList;
import java.util.List;

import org.libsdl.app.SDLActivity;
import org.libsdl.app.SDLSurface;

public class PlxActivity extends SDLActivity {
    private static final String TAG = "PlxNative";
    private static final Object sVideoLock = new Object();
    private static Surface sVideoSurface;
    private static SurfaceView sVideoView;

    // SDLActivity loads these in order and enters SDL_main in the last one.
    @Override
    protected String[] getLibraries() {
        return new String[] {"SDL2", "SDL2_ttf", "main"};
    }

    // SDL_main's arguments: each intent extra the native harness reads, as key=value. SDL asks once,
    // before it starts the native thread, so a new clip needs a fresh start (am start -S).
    @Override
    protected String[] getArguments() {
        List<String> args = new ArrayList<>();
        Intent intent = getIntent();
        for (String key : new String[] {"clip", "vc", "ac", "fps"}) {
            String value = intent == null ? null : intent.getStringExtra(key);
            if (value != null) {
                args.add(key + "=" + value);
            }
        }
        return args.toArray(new String[0]);
    }

    // The video SurfaceView goes under SDL's: SDL's is a media overlay, so it composes above this
    // one, and it is translucent, so the video shows wherever the UI draws nothing.
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (mBrokenLibraries || mLayout == null) {
            return;
        }
        SurfaceView video = new SurfaceView(this);
        video.getHolder().addCallback(new SurfaceHolder.Callback() {
            @Override
            public void surfaceCreated(SurfaceHolder holder) {
                setVideoSurface(holder.getSurface());
            }

            @Override
            public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {}

            @Override
            public void surfaceDestroyed(SurfaceHolder holder) {
                setVideoSurface(null);
            }
        });
        mLayout.addView(video, 0, new RelativeLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        sVideoView = video;
    }

    // The UI is drawn at 1920x1080 on every stick; the display hardware scales it to the panel.
    @Override
    protected SDLSurface createSDLSurface(Context context) {
        SDLSurface surface = new SDLSurface(context);
        surface.getHolder().setFixedSize(1920, 1080);
        surface.getHolder().setFormat(PixelFormat.TRANSLUCENT);
        surface.setZOrderMediaOverlay(true);
        return surface;
    }

    // A key the native side maps is consumed here, so SDL never sees it; SDL's BACK would finish the
    // Activity. Unmapped keys (volume, ...) take SDL's and Android's usual path.
    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (!mBrokenLibraries && nativeKey(event.getKeyCode(), event.getAction(), event.getRepeatCount())) {
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    private static native boolean nativeKey(int keyCode, int action, int repeatCount);

    private static void setVideoSurface(Surface surface) {
        synchronized (sVideoLock) {
            sVideoSurface = surface;
            sVideoLock.notifyAll();
        }
    }

    // Called by the native sink on its load thread: a started MediaTek decoder for mime that renders
    // to the video SurfaceView, or null (logged). The native side feeds and drains it itself.
    static MediaCodec openVideoDecoder(String mime, int width, int height) {
        String name = "OMX.MTK.VIDEO.DECODER." + ("video/hevc".equals(mime) ? "HEVC" : "AVC");
        if (!hasDecoder(name, mime)) {
            Log.e(TAG, "video: no decoder " + name + " for " + mime);
            return null;
        }
        Surface surface;
        synchronized (sVideoLock) {
            long until = SystemClock.uptimeMillis() + 3000;
            while (sVideoSurface == null && SystemClock.uptimeMillis() < until) {
                try {
                    sVideoLock.wait(100);
                } catch (InterruptedException e) {
                    return null;
                }
            }
            surface = sVideoSurface;
        }
        if (surface == null) {
            Log.e(TAG, "video: the video surface never appeared");
            return null;
        }
        MediaCodec codec = null;
        try {
            codec = MediaCodec.createByCodecName(name);
            MediaFormat format = MediaFormat.createVideoFormat(mime, width, height);
            // Room for a whole access unit at the largest raster; the default can be smaller than a 4K keyframe.
            format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, width * height * 3 / 4);
            codec.configure(format, surface, null, 0);
            codec.start();
            return codec;
        } catch (Exception e) {
            Log.e(TAG, "video: " + name + " did not start", e);
            if (codec != null) {
                codec.release();
            }
            return null;
        }
    }

    private static boolean hasDecoder(String name, String mime) {
        for (MediaCodecInfo info : new MediaCodecList(MediaCodecList.ALL_CODECS).getCodecInfos()) {
            if (info.isEncoder() || !info.getName().equals(name)) {
                continue;
            }
            for (String type : info.getSupportedTypes()) {
                if (type.equalsIgnoreCase(mime)) {
                    return true;
                }
            }
        }
        return false;
    }

    // Called by the native sink when the decoder's output format changes: the SurfaceView is resized
    // to the picture's aspect ratio, centred, since the decoder scales the picture to fill it.
    static void videoFormatChanged(MediaCodec codec) {
        MediaFormat format = codec.getOutputFormat();
        Log.i(TAG, "video: output format " + format);
        int width = format.getInteger(MediaFormat.KEY_WIDTH);
        int height = format.getInteger(MediaFormat.KEY_HEIGHT);
        if (format.containsKey("crop-right") && format.containsKey("crop-left")
                && format.containsKey("crop-bottom") && format.containsKey("crop-top")) {
            width = format.getInteger("crop-right") - format.getInteger("crop-left") + 1;
            height = format.getInteger("crop-bottom") - format.getInteger("crop-top") + 1;
        }
        final SurfaceView video = sVideoView;
        final int w = width;
        final int h = height;
        if (video != null && w > 0 && h > 0) {
            video.post(() -> fitVideo(video, w, h));
        }
    }

    private static void fitVideo(SurfaceView video, int width, int height) {
        ViewGroup parent = (ViewGroup) video.getParent();
        if (parent == null || parent.getWidth() == 0 || parent.getHeight() == 0) {
            return;
        }
        int w = parent.getWidth();
        int h = (int) ((long) w * height / width);
        if (h > parent.getHeight()) {
            h = parent.getHeight();
            w = (int) ((long) h * width / height);
        }
        RelativeLayout.LayoutParams params = new RelativeLayout.LayoutParams(w, h);
        params.addRule(RelativeLayout.CENTER_IN_PARENT);
        video.setLayoutParams(params);
        Log.i(TAG, "video: view " + w + "x" + h + " for a " + width + "x" + height + " picture");
    }
}
