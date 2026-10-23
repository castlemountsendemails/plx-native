package com.beb.plxnative;

import android.content.Context;
import android.view.KeyEvent;

import org.libsdl.app.SDLActivity;
import org.libsdl.app.SDLSurface;

public class PlxActivity extends SDLActivity {
    // SDLActivity loads these in order and enters SDL_main in the last one.
    @Override
    protected String[] getLibraries() {
        return new String[] {"SDL2", "SDL2_ttf", "main"};
    }

    // The UI is drawn at 1920x1080 on every stick; the display hardware scales it to the panel.
    @Override
    protected SDLSurface createSDLSurface(Context context) {
        SDLSurface surface = new SDLSurface(context);
        surface.getHolder().setFixedSize(1920, 1080);
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
}
