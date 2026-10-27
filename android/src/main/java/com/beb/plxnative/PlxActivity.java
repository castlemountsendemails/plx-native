package com.beb.plxnative;

import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.graphics.PixelFormat;
import android.media.AudioAttributes;
import android.media.AudioDeviceInfo;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaFormat;
import android.os.Bundle;
import android.os.SystemClock;
import android.provider.Settings;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Log;
import android.view.KeyEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.ViewGroup;
import android.widget.RelativeLayout;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

import org.libsdl.app.SDLActivity;
import org.libsdl.app.SDLSurface;

public class PlxActivity extends SDLActivity {
    private static final String TAG = "PlxNative";
    private static final Object sVideoLock = new Object();
    private static Surface sVideoSurface;
    private static SurfaceView sVideoView;
    private static Context sContext;

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
        for (String key : new String[] {"clip", "vc", "ac", "fps", "size", "secure"}) {
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
        sContext = getApplicationContext();
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
            // The engine passes the clip's largest raster (3840x2160 for a 1080p clip); the HD and 3rd gen
            // AVC decoder rejects that in configure, so the size is capped to what the decoder reports.
            MediaCodecInfo.VideoCapabilities caps = codec.getCodecInfo().getCapabilitiesForType(mime).getVideoCapabilities();
            if (!caps.isSizeSupported(width, height)) {
                int w = Math.min(width, caps.getSupportedWidths().getUpper());
                int h = Math.min(height, caps.getSupportedHeights().getUpper());
                Log.w(TAG, "video: " + name + " rejects " + width + "x" + height + ", configuring " + w + "x" + h
                        + " (widths " + caps.getSupportedWidths() + ", heights " + caps.getSupportedHeights() + ")");
                width = w;
                height = h;
            }
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

    private static final String AAC = "audio/mp4a-latm";
    // The decoder downmixes 7.1 to 5.1, the most PCM channels the sticks list for HDMI.
    private static final int AAC_MAX_CHANNELS = 6;

    // Called by the native audio sink: a streaming track, or null (logged). AC-3 goes out as it is
    // (HDMI passthrough), as 5.1 or else stereo; PCM is what the AAC decoder produced.
    static AudioTrack openAudioTrack(int encoding, int sampleRate, int channels) {
        boolean pcm = encoding == AudioFormat.ENCODING_PCM_16BIT || encoding == AudioFormat.ENCODING_PCM_FLOAT;
        int[] masks = pcm ? new int[] {channelMask(channels)}
                : new int[] {AudioFormat.CHANNEL_OUT_5POINT1, AudioFormat.CHANNEL_OUT_STEREO};
        for (int mask : masks) {
            int min = AudioTrack.getMinBufferSize(sampleRate, mask, encoding);
            // PCM: four times the minimum. AC-3: about half a second at 640 kbit/s.
            int size = pcm ? Math.max(min, 1) * 4 : Math.max(min, 40000);
            try {
                AudioTrack track = new AudioTrack(
                        new AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_MEDIA)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE)
                                .build(),
                        new AudioFormat.Builder()
                                .setEncoding(encoding)
                                .setSampleRate(sampleRate)
                                .setChannelMask(mask)
                                .build(),
                        size, AudioTrack.MODE_STREAM, AudioManager.AUDIO_SESSION_ID_GENERATE);
                if (track.getState() == AudioTrack.STATE_INITIALIZED) {
                    Log.i(TAG, "audio: track encoding " + encoding + " " + sampleRate + " Hz mask 0x"
                            + Integer.toHexString(mask) + " buffer " + size + " (min " + min + ") routed to "
                            + routedTo(track));
                    return track;
                }
                track.release();
                Log.w(TAG, "audio: track encoding " + encoding + " mask 0x" + Integer.toHexString(mask) + " did not initialize");
            } catch (Exception e) {
                Log.w(TAG, "audio: track encoding " + encoding + " mask 0x" + Integer.toHexString(mask) + " refused", e);
            }
        }
        return null;
    }

    private static int channelMask(int channels) {
        switch (channels) {
            case 1: return AudioFormat.CHANNEL_OUT_MONO;
            case 6: return AudioFormat.CHANNEL_OUT_5POINT1;
            case 8: return AudioFormat.CHANNEL_OUT_7POINT1_SURROUND;
            default: return AudioFormat.CHANNEL_OUT_STEREO;
        }
    }

    private static String routedTo(AudioTrack track) {
        AudioDeviceInfo device = track.getRoutedDevice();
        return device == null ? "nothing yet" : "type " + device.getType() + " " + device.getProductName();
    }

    // Called by the native audio sink: a started AAC decoder for raw frames configured by csd (an
    // AudioSpecificConfig), or null (logged).
    static MediaCodec openAudioDecoder(int sampleRate, int channels, byte[] csd) {
        MediaCodec codec = null;
        try {
            codec = MediaCodec.createDecoderByType(AAC);
            MediaFormat format = MediaFormat.createAudioFormat(AAC, sampleRate, channels);
            format.setByteBuffer("csd-0", ByteBuffer.wrap(csd));
            format.setInteger(MediaFormat.KEY_AAC_MAX_OUTPUT_CHANNEL_COUNT, AAC_MAX_CHANNELS);
            codec.configure(format, null, null, 0);
            codec.start();
            Log.i(TAG, "audio: " + codec.getName() + " configured with " + format);
            return codec;
        } catch (Exception e) {
            Log.e(TAG, "audio: no AAC decoder started", e);
            if (codec != null) {
                codec.release();
            }
            return null;
        }
    }

    // {sample rate, channel count, PCM encoding} of the decoder's output; 16-bit unless it says otherwise.
    static int[] audioOutputFormat(MediaCodec codec) {
        MediaFormat format = codec.getOutputFormat();
        Log.i(TAG, "audio: output format " + format);
        int encoding = format.containsKey("pcm-encoding") ? format.getInteger("pcm-encoding") : AudioFormat.ENCODING_PCM_16BIT;
        return new int[] {
                format.getInteger(MediaFormat.KEY_SAMPLE_RATE), format.getInteger(MediaFormat.KEY_CHANNEL_COUNT), encoding};
    }

    private static List<AudioDeviceInfo> hdmiOutputs() {
        List<AudioDeviceInfo> hdmi = new ArrayList<>();
        AudioManager audio = (AudioManager) sContext.getSystemService(Context.AUDIO_SERVICE);
        for (AudioDeviceInfo device : audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
            if (device.getType() == AudioDeviceInfo.TYPE_HDMI || device.getType() == AudioDeviceInfo.TYPE_HDMI_ARC) {
                hdmi.add(device);
            }
        }
        return hdmi;
    }

    // 1 if an HDMI output lists the encoding, 0 if they list others but not it, -1 if none lists any
    // (an empty list means the output takes any encoding).
    static int sinkTakes(int encoding) {
        int takes = -1;
        for (AudioDeviceInfo device : hdmiOutputs()) {
            int[] encodings = device.getEncodings();
            for (int e : encodings) {
                if (e == encoding) {
                    return 1;
                }
            }
            if (encodings.length > 0) {
                takes = 0;
            }
        }
        return takes;
    }

    // Logs what each source reports about the audio outputs, so each stick's HDMI formats are on record.
    static void reportAudio() {
        AudioManager audio = (AudioManager) sContext.getSystemService(Context.AUDIO_SERVICE);
        for (AudioDeviceInfo device : audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
            Log.i(TAG, "audio: output type " + device.getType() + " '" + device.getProductName() + "' encodings "
                    + Arrays.toString(device.getEncodings()) + " channel counts " + Arrays.toString(device.getChannelCounts())
                    + " rates " + Arrays.toString(device.getSampleRates()));
        }
        Intent plug = sContext.registerReceiver(null, new IntentFilter(AudioManager.ACTION_HDMI_AUDIO_PLUG));
        if (plug == null) {
            Log.i(TAG, "audio: no HDMI plug broadcast");
        } else {
            Log.i(TAG, "audio: HDMI plug state " + plug.getIntExtra(AudioManager.EXTRA_AUDIO_PLUG_STATE, -1)
                    + " max channels " + plug.getIntExtra(AudioManager.EXTRA_MAX_CHANNEL_COUNT, -1)
                    + " encodings " + Arrays.toString(plug.getIntArrayExtra(AudioManager.EXTRA_ENCODINGS)));
        }
        Log.i(TAG, "audio: audio_platform_capabilities "
                + Settings.Global.getString(sContext.getContentResolver(), "audio_platform_capabilities"));
        for (MediaCodecInfo info : new MediaCodecList(MediaCodecList.ALL_CODECS).getCodecInfos()) {
            if (info.isEncoder()) {
                continue;
            }
            for (String type : info.getSupportedTypes()) {
                if (type.equalsIgnoreCase(AAC)) {
                    Log.i(TAG, "audio: AAC decoder " + info.getName() + " max channels "
                            + info.getCapabilitiesForType(AAC).getAudioCapabilities().getMaxInputChannelCount());
                }
            }
        }
    }

    // The Keystore sealing behind tv::secure: an AES-256-GCM key under the alias, made on the first
    // seal. Every failure (missing key, tampered blob, Keystore error) is caught here and comes back
    // as null or false, so no exception reaches JNI.
    private static final int GCM_TAG_BITS = 128;

    // The IV (12 bytes) followed by ciphertext + tag, or null.
    static byte[] sealSecret(byte[] alias, byte[] plain) {
        try {
            String name = new String(alias, StandardCharsets.UTF_8);
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            SecretKey key = (SecretKey) store.getKey(name, null);
            if (key == null) {
                KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
                generator.init(new KeyGenParameterSpec.Builder(name, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                        .setKeySize(256)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .build());
                key = generator.generateKey();
            }
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, key);
            byte[] iv = cipher.getIV();
            byte[] data = cipher.doFinal(plain);
            byte[] out = new byte[iv.length + data.length];
            System.arraycopy(iv, 0, out, 0, iv.length);
            System.arraycopy(data, 0, out, iv.length, data.length);
            return out;
        } catch (Exception e) {
            Log.e(TAG, "secure: seal failed: " + e);
            return null;
        }
    }

    // The plaintext, or null for a missing key, a tampered blob or any Keystore error.
    static byte[] openSecret(byte[] alias, byte[] iv, byte[] data) {
        try {
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            SecretKey key = (SecretKey) store.getKey(new String(alias, StandardCharsets.UTF_8), null);
            if (key == null) {
                Log.i(TAG, "secure: open: no key");
                return null;
            }
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(GCM_TAG_BITS, iv));
            return cipher.doFinal(data);
        } catch (Exception e) {
            Log.e(TAG, "secure: open failed: " + e);
            return null;
        }
    }

    // Whether the key is gone afterwards; a key that was never there counts.
    static boolean removeSecret(byte[] alias) {
        try {
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            store.deleteEntry(new String(alias, StandardCharsets.UTF_8));
            return true;
        } catch (Exception e) {
            Log.e(TAG, "secure: remove failed: " + e);
            return false;
        }
    }
}
