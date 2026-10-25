//! Every JNI call. The static natives `PlxActivity` calls run on the Activity's UI thread and the SDL
//! thread reads what they leave, so that state sits behind one mutex. `VideoCodec` drives the
//! `MediaCodec` that `PlxActivity.openVideoDecoder` starts, from any thread.

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use ::jni::objects::{GlobalRef, JByteBuffer, JClass, JString, JValue};
use ::jni::sys::{jint, JNI_VERSION_1_6};
use ::jni::{JNIEnv, JavaVM};

use crate::keys::{self, Edge, Key, Keys, Press};
use crate::skeleton::{log, LOG_ERROR, LOG_INFO};

struct Bridge {
    keys: Keys,
    readout: String,
    back: bool,
}

static BRIDGE: Mutex<Bridge> = Mutex::new(Bridge {
    keys: Keys::new(),
    readout: String::new(),
    back: false,
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

pub struct VideoCodec {
    codec: GlobalRef,
    info: GlobalRef,
    pub name: String,
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

impl VideoCodec {
    pub fn open(mime: &str, width: i32, height: i32) -> Option<Self> {
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
            if codec.is_null() {
                return Ok(None);
            }
            let name = JString::from(env.call_method(&codec, "getName", "()Ljava/lang/String;", &[])?.l()?);
            let name = env.get_string(&name)?.into();
            let info = env.new_object("android/media/MediaCodec$BufferInfo", "()V", &[])?;
            Ok(Some(Self { codec: env.new_global_ref(codec)?, info: env.new_global_ref(info)?, name }))
        })
        .flatten()
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

    /// The next decoded frame, if one is ready. A format change resizes the video view first.
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

    pub fn flush(&self) -> Option<()> {
        with_env("flush the decoder", |env| env.call_method(&self.codec, "flush", "()V", &[]).map(drop))
    }

    /// Separate calls, so a failed `stop` (its exception cleared) still releases the decoder.
    pub fn close(self) {
        with_env("stop the decoder", |env| env.call_method(&self.codec, "stop", "()V", &[]).map(drop));
        with_env("release the decoder", |env| env.call_method(&self.codec, "release", "()V", &[]).map(drop));
    }
}
