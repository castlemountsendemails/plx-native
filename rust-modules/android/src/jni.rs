//! Every JNI call. The static natives `PlxActivity` calls run on the Activity's UI thread and the SDL
//! thread reads what they leave, so that state sits behind one mutex. `Codec` drives a `MediaCodec`
//! that `PlxActivity` starts, and `Track` an `AudioTrack`, from any thread.

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use ::jni::objects::{GlobalRef, JByteArray, JByteBuffer, JClass, JIntArray, JObject, JString, JValue};
use ::jni::sys::{jint, JNI_VERSION_1_6};
use ::jni::{JNIEnv, JavaVM};

use crate::keys::{self, Edge, Key, Keys, Press};
use crate::skeleton::{log, LOG_ERROR, LOG_INFO};

struct Bridge {
    keys: Keys,
    readout: String,
    back: bool,
    presses: Vec<Press>,
}

static BRIDGE: Mutex<Bridge> = Mutex::new(Bridge {
    keys: Keys::new(),
    readout: String::new(),
    back: false,
    presses: Vec::new(),
});

fn bridge() -> MutexGuard<'static, Bridge> {
    BRIDGE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

impl Bridge {
    fn record(&mut self, presses: Vec<Press>) {
        for Press { key, edge, code } in presses {
            self.back |= key == Key::Back && edge == Edge::Down;
            self.presses.push(Press { key, edge, code });
            self.readout = format!("keycode {code} -> {key:?} ({edge:?})");
            log(LOG_INFO, &self.readout);
        }
    }
}

/// `static native boolean nativeKey(int keyCode, int action, int repeatCount)`. True for a mapped
/// key, which Java then consumes so SDL never sees it (SDL's BACK would finish the Activity).
#[no_mangle]
pub extern "system" fn Java_com_beb_plxnative_PlxActivity_nativeKey(
    _env: *mut c_void,
    _class: *mut c_void,
    code: i32,
    action: i32,
    repeat: i32,
) -> u8 {
    let mut bridge = bridge();
    let presses = bridge.keys.event(code, action, repeat, now_ms());
    bridge.record(presses);
    u8::from(keys::map(code).is_some())
}

pub fn tick() {
    let mut bridge = bridge();
    let presses = bridge.keys.tick(now_ms());
    bridge.record(presses);
}

/// The last press, as the skeleton draws it; empty before the first.
pub fn readout() -> String {
    bridge().readout.clone()
}

/// Whether BACK was pressed since the last call.
pub fn take_back() -> bool {
    std::mem::take(&mut bridge().back)
}

/// Every press since the last call.
pub fn take_presses() -> Vec<Press> {
    std::mem::take(&mut bridge().presses)
}

static VM: OnceLock<JavaVM> = OnceLock::new();
static ACTIVITY: OnceLock<GlobalRef> = OnceLock::new();

/// Runs once, on the thread that loads `libmain.so`. `PlxActivity` is looked up here because
/// `FindClass` on a native thread sees only the system class loader.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _reserved: *mut c_void) -> jint {
    if let Ok(mut env) = vm.get_env() {
        match env.find_class("com/beb/plxnative/PlxActivity").and_then(|class| env.new_global_ref(class)) {
            Ok(class) => {
                let _ = ACTIVITY.set(class);
            }
            Err(e) => fail(&mut env, "find PlxActivity", e),
        }
    }
    let _ = VM.set(vm);
    JNI_VERSION_1_6
}

fn fail(env: &mut JNIEnv, what: &str, e: ::jni::errors::Error) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    log(LOG_ERROR, &format!("jni: {what}: {e}"));
}

/// Runs `f` on this thread's JNIEnv, attaching the thread for its lifetime if it is not a Java
/// thread. The local frame frees `f`'s local references, which the SDL thread never returns to
/// Java to free. `None` means it failed, and the failure is logged.
fn with_env<T>(what: &str, f: impl FnOnce(&mut JNIEnv) -> ::jni::errors::Result<T>) -> Option<T> {
    let mut env = VM.get()?.attach_current_thread_permanently().ok()?;
    match env.with_local_frame(16, f) {
        Ok(value) => Some(value),
        Err(e) => {
            fail(&mut env, what, e);
            None
        }
    }
}

fn activity() -> Option<&'static JClass<'static>> {
    ACTIVITY.get().map(|class| <&JClass>::from(class.as_obj()))
}

const BUFFER_FLAG_END_OF_STREAM: i32 = 4;
const INFO_OUTPUT_FORMAT_CHANGED: i32 = -2;

pub struct Codec {
    codec: GlobalRef,
    info: GlobalRef,
    pub name: String,
    video: bool,
}

pub enum Input {
    Queued,
    Full,
    /// The access unit is larger than the input buffer, whose capacity this is.
    TooBig(usize),
}

pub enum Output {
    None,
    FormatChanged,
    Frame { index: i32, pts_us: i64, end: bool },
}

impl Codec {
    pub fn open_video(mime: &str, width: i32, height: i32) -> Option<Self> {
        let class = activity()?;
        with_env("open the video decoder", |env| {
            let mime = env.new_string(mime)?;
            let codec = env
                .call_static_method(
                    class,
                    "openVideoDecoder",
                    "(Ljava/lang/String;II)Landroid/media/MediaCodec;",
                    &[JValue::Object(&mime), JValue::Int(width), JValue::Int(height)],
                )?
                .l()?;
            Self::wrap(env, codec, true)
        })
        .flatten()
    }

    /// An AAC decoder for raw frames, configured by the AudioSpecificConfig `csd`.
    pub fn open_aac(rate: i32, channels: i32, csd: &[u8]) -> Option<Self> {
        let class = activity()?;
        with_env("open the audio decoder", |env| {
            let csd = env.byte_array_from_slice(csd)?;
            let codec = env
                .call_static_method(
                    class,
                    "openAudioDecoder",
                    "(II[B)Landroid/media/MediaCodec;",
                    &[JValue::Int(rate), JValue::Int(channels), JValue::Object(&csd)],
                )?
                .l()?;
            Self::wrap(env, codec, false)
        })
        .flatten()
    }

    fn wrap(env: &mut JNIEnv, codec: JObject, video: bool) -> ::jni::errors::Result<Option<Self>> {
        if codec.is_null() {
            return Ok(None);
        }
        let name = JString::from(env.call_method(&codec, "getName", "()Ljava/lang/String;", &[])?.l()?);
        let name = env.get_string(&name)?.into();
        let info = env.new_object("android/media/MediaCodec$BufferInfo", "()V", &[])?;
        Ok(Some(Self { codec: env.new_global_ref(codec)?, info: env.new_global_ref(info)?, name, video }))
    }

    /// Copies `data` into a free input buffer and queues it; `end` queues end of stream instead.
    pub fn queue(&self, data: &[u8], pts_us: i64, end: bool) -> Option<Input> {
        with_env("queue an input buffer", |env| {
            let index = env.call_method(&self.codec, "dequeueInputBuffer", "(J)I", &[JValue::Long(0)])?.i()?;
            if index < 0 {
                return Ok(Input::Full);
            }
            let buffer = JByteBuffer::from(
                env.call_method(&self.codec, "getInputBuffer", "(I)Ljava/nio/ByteBuffer;", &[JValue::Int(index)])?
                    .l()?,
            );
            let capacity = env.get_direct_buffer_capacity(&buffer)?;
            // An AU that does not fit still returns its dequeued buffer, empty.
            let len = if data.len() <= capacity { data.len() } else { 0 };
            let address = env.get_direct_buffer_address(&buffer)?;
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), address, len) };
            let flags = if end { BUFFER_FLAG_END_OF_STREAM } else { 0 };
            env.call_method(
                &self.codec,
                "queueInputBuffer",
                "(IIIJI)V",
                &[JValue::Int(index), JValue::Int(0), JValue::Int(len as i32), JValue::Long(pts_us), JValue::Int(flags)],
            )?;
            Ok(if len == data.len() { Input::Queued } else { Input::TooBig(capacity) })
        })
    }

    /// The next decoded frame, if one is ready. A video format change resizes the video view first.
    pub fn dequeue(&self) -> Option<Output> {
        let class = activity()?;
        with_env("dequeue an output buffer", |env| {
            let index = env
                .call_method(
                    &self.codec,
                    "dequeueOutputBuffer",
                    "(Landroid/media/MediaCodec$BufferInfo;J)I",
                    &[JValue::Object(self.info.as_obj()), JValue::Long(0)],
                )?
                .i()?;
            if index == INFO_OUTPUT_FORMAT_CHANGED && !self.video {
                return Ok(Output::FormatChanged);
            }
            if index == INFO_OUTPUT_FORMAT_CHANGED {
                env.call_static_method(
                    class,
                    "videoFormatChanged",
                    "(Landroid/media/MediaCodec;)V",
                    &[JValue::Object(self.codec.as_obj())],
                )?;
                return Ok(Output::FormatChanged);
            }
            if index < 0 {
                return Ok(Output::None);
            }
            let pts_us = env.get_field(&self.info, "presentationTimeUs", "J")?.j()?;
            let flags = env.get_field(&self.info, "flags", "I")?.i()?;
            Ok(Output::Frame { index, pts_us, end: flags & BUFFER_FLAG_END_OF_STREAM != 0 })
        })
    }

    /// Shows the frame at `render_ns` (`CLOCK_MONOTONIC`), or drops it with `None`.
    pub fn release(&self, index: i32, render_ns: Option<i64>) -> Option<()> {
        with_env("release an output buffer", |env| {
            match render_ns {
                Some(at) => env.call_method(&self.codec, "releaseOutputBuffer", "(IJ)V", &[JValue::Int(index), JValue::Long(at)]),
                None => env.call_method(&self.codec, "releaseOutputBuffer", "(IZ)V", &[JValue::Int(index), JValue::Bool(0)]),
            }?;
            Ok(())
        })
    }

    /// A copy of the bytes in the output buffer `dequeue` last returned.
    pub fn output(&self, index: i32) -> Option<Vec<u8>> {
        with_env("read an output buffer", |env| {
            let offset = env.get_field(&self.info, "offset", "I")?.i()?.max(0) as usize;
            let size = env.get_field(&self.info, "size", "I")?.i()?.max(0) as usize;
            let buffer = JByteBuffer::from(
                env.call_method(&self.codec, "getOutputBuffer", "(I)Ljava/nio/ByteBuffer;", &[JValue::Int(index)])?
                    .l()?,
            );
            let capacity = env.get_direct_buffer_capacity(&buffer)?;
            let address = env.get_direct_buffer_address(&buffer)?;
            let end = (offset + size).min(capacity);
            let start = offset.min(end);
            Ok(unsafe { std::slice::from_raw_parts(address.add(start), end - start) }.to_vec())
        })
    }

    /// The decoded audio's sample rate, channel count and PCM encoding, after a format change.
    pub fn audio_format(&self) -> Option<(i32, i32, i32)> {
        let class = activity()?;
        with_env("read the audio output format", |env| {
            let array = JIntArray::from(
                env.call_static_method(
                    class,
                    "audioOutputFormat",
                    "(Landroid/media/MediaCodec;)[I",
                    &[JValue::Object(self.codec.as_obj())],
                )?
                .l()?,
            );
            let mut format = [0; 3];
            env.get_int_array_region(&array, 0, &mut format)?;
            Ok((format[0], format[1], format[2]))
        })
    }

    pub fn flush(&self) -> Option<()> {
        with_env("flush the decoder", |env| env.call_method(&self.codec, "flush", "()V", &[]).map(drop))
    }

    /// Separate calls, so a failed `stop` (its exception cleared) still releases the decoder.
    pub fn close(self) {
        with_env("stop the decoder", |env| env.call_method(&self.codec, "stop", "()V", &[]).map(drop));
        with_env("release the decoder", |env| env.call_method(&self.codec, "release", "()V", &[]).map(drop));
    }
}

const WRITE_NON_BLOCKING: i32 = 1;

/// A streaming `AudioTrack` that `PlxActivity.openAudioTrack` built.
pub struct Track {
    track: GlobalRef,
    stamp: GlobalRef,
}

impl Track {
    pub fn open(encoding: i32, rate: i32, channels: i32) -> Option<Self> {
        let class = activity()?;
        with_env("open the audio track", |env| {
            let track = env
                .call_static_method(
                    class,
                    "openAudioTrack",
                    "(III)Landroid/media/AudioTrack;",
                    &[JValue::Int(encoding), JValue::Int(rate), JValue::Int(channels)],
                )?
                .l()?;
            if track.is_null() {
                return Ok(None);
            }
            let stamp = env.new_object("android/media/AudioTimestamp", "()V", &[])?;
            Ok(Some(Self { track: env.new_global_ref(track)?, stamp: env.new_global_ref(stamp)? }))
        })
        .flatten()
    }

    /// Writes what fits without waiting and returns how many bytes that was.
    pub fn write(&self, data: &[u8]) -> Option<usize> {
        with_env("write to the audio track", |env| {
            // AudioTrack only reads the buffer, and drops it before this call returns.
            let buffer = unsafe { env.new_direct_byte_buffer(data.as_ptr().cast_mut(), data.len())? };
            let written = env
                .call_method(
                    &self.track,
                    "write",
                    "(Ljava/nio/ByteBuffer;II)I",
                    &[JValue::Object(&buffer), JValue::Int(data.len() as i32), JValue::Int(WRITE_NON_BLOCKING)],
                )?
                .i()?;
            Ok(usize::try_from(written).ok())
        })
        .flatten()
    }

    fn call(&self, what: &str, method: &str) -> Option<()> {
        with_env(what, |env| env.call_method(&self.track, method, "()V", &[]).map(drop))
    }

    pub fn play(&self) -> Option<()> {
        self.call("play the audio track", "play")
    }

    pub fn pause(&self) -> Option<()> {
        self.call("pause the audio track", "pause")
    }

    /// `getTimestamp`'s frame and the `CLOCK_MONOTONIC` time it was heard, or `Some(None)` while
    /// the track has none.
    pub fn timestamp(&self) -> Option<Option<(i64, i64)>> {
        with_env("read the audio timestamp", |env| {
            let stamp = JValue::Object(self.stamp.as_obj());
            if !env.call_method(&self.track, "getTimestamp", "(Landroid/media/AudioTimestamp;)Z", &[stamp])?.z()? {
                return Ok(None);
            }
            let frame = env.get_field(&self.stamp, "framePosition", "J")?.j()?;
            let at = env.get_field(&self.stamp, "nanoTime", "J")?.j()?;
            Ok(Some((frame, at)))
        })
    }

    /// Frames played since the track was built; Android keeps the count as an unsigned 32 bits.
    pub fn head(&self) -> Option<i64> {
        with_env("read the audio head position", |env| {
            let head = env.call_method(&self.track, "getPlaybackHeadPosition", "()I", &[])?.i()?;
            Ok(i64::from(head as u32))
        })
    }

    pub fn underruns(&self) -> Option<i32> {
        with_env("read the audio underruns", |env| env.call_method(&self.track, "getUnderrunCount", "()I", &[])?.i())
    }

    /// Separate calls, so one that fails (its exception cleared) still lets the track be released.
    pub fn close(self) {
        self.call("pause the audio track", "pause");
        self.call("flush the audio track", "flush");
        self.call("release the audio track", "release");
    }
}

/// Whether an HDMI output lists `encoding`: 1 yes, 0 no, -1 when none lists any encodings.
pub fn sink_takes(encoding: i32) -> i32 {
    let Some(class) = activity() else { return -1 };
    with_env("ask HDMI for an encoding", |env| env.call_static_method(class, "sinkTakes", "(I)I", &[JValue::Int(encoding)])?.i())
        .unwrap_or(-1)
}

/// Logs what the outputs, the HDMI plug broadcast and Fire OS's audio settings report, and the
/// stick's AAC decoders.
pub fn audio_report() {
    if let Some(class) = activity() {
        with_env("report the audio outputs", |env| env.call_static_method(class, "reportAudio", "()V", &[]).map(drop));
    }
}

/// `PlxActivity.sealSecret`: the Keystore IV (12 bytes) followed by ciphertext + tag.
pub fn seal_secret(alias: &str, plain: &[u8]) -> Option<Vec<u8>> {
    let class = activity()?;
    with_env("seal a secret", |env| {
        let alias = env.byte_array_from_slice(alias.as_bytes())?;
        let plain = env.byte_array_from_slice(plain)?;
        let sealed = env.call_static_method(
            class,
            "sealSecret",
            "([B[B)[B",
            &[JValue::Object(&alias), JValue::Object(&plain)],
        )?
        .l()?;
        bytes(env, sealed)
    })
    .flatten()
}

/// `PlxActivity.openSecret`; `None` for a missing key, a tampered blob or a Keystore error.
pub fn open_secret(alias: &str, iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let class = activity()?;
    with_env("open a secret", |env| {
        let alias = env.byte_array_from_slice(alias.as_bytes())?;
        let iv = env.byte_array_from_slice(iv)?;
        let data = env.byte_array_from_slice(data)?;
        let plain = env.call_static_method(
            class,
            "openSecret",
            "([B[B[B)[B",
            &[JValue::Object(&alias), JValue::Object(&iv), JValue::Object(&data)],
        )?
        .l()?;
        bytes(env, plain)
    })
    .flatten()
}

/// `PlxActivity.removeSecret`: whether the key is gone.
pub fn remove_secret(alias: &str) -> bool {
    let Some(class) = activity() else { return false };
    with_env("remove a secret", |env| {
        let alias = env.byte_array_from_slice(alias.as_bytes())?;
        env.call_static_method(class, "removeSecret", "([B)Z", &[JValue::Object(&alias)])?.z()
    })
    .unwrap_or(false)
}

fn bytes(env: &mut JNIEnv, array: JObject) -> ::jni::errors::Result<Option<Vec<u8>>> {
    if array.is_null() {
        return Ok(None);
    }
    env.convert_byte_array(JByteArray::from(array)).map(Some)
}
