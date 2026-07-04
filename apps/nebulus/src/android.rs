//! Android `NativeActivity` and `UsbManager` integration.

use std::{
    os::fd::{FromRawFd as _, OwnedFd},
    sync::{Condvar, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use android_activity::AndroidApp;
use jni::{
    jni_sig, jni_str,
    objects::{Global, JByteArray, JClass, JFloatArray, JObject, JString},
    vm::JavaVM,
    EnvUnowned, JValue,
};
use nusb::MaybeFuture as _;

use crate::runtime::UsbDeviceInfo;

static ANDROID_APP: OnceLock<Mutex<Option<AndroidApp>>> = OnceLock::new();
static KEY_FILE_RESULT: OnceLock<Mutex<Option<Result<SelectedKeyFile, String>>>> = OnceLock::new();
static KEY_FILE_CONTEXT: OnceLock<Mutex<Option<eframe::egui::Context>>> = OnceLock::new();
#[derive(Default)]
struct VpnRequestState {
    waiting: bool,
    result: Option<Result<OpenedVpn, String>>,
}

type VpnResult = (Mutex<VpnRequestState>, Condvar);
static VPN_RESULT: OnceLock<VpnResult> = OnceLock::new();
const USB_PERMISSION_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct SelectedKeyFile {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
}

/// USB device and Java connection retained for the lifetime of `nusb` access.
pub(crate) struct OpenedUsbDevice {
    pub(crate) device: nusb::Device,
    pub(crate) info: UsbDeviceInfo,
    pub(crate) connection: Global<JObject<'static>>,
}

pub(crate) struct OpenedVpn {
    pub(crate) fd: i32,
    pub(crate) interface_name: String,
}

/// Java SurfaceTexture and its native producer window used by MediaCodec.
pub(crate) struct AndroidVideoSurface {
    app: AndroidApp,
    vm: JavaVM,
    surface: Global<JObject<'static>>,
    window: ndk::native_window::NativeWindow,
}

impl AndroidVideoSurface {
    pub(crate) fn create(texture_id: u32) -> Result<Self, String> {
        let app = app()?;
        let vm = java_vm(&app)?;
        let (surface, window) = vm
            .attach_current_thread(|env| {
                let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
                // SAFETY: android-activity owns this activity global reference.
                let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
                let local_surface = env
                    .call_method(
                        &activity,
                        jni_str!("createVideoSurface"),
                        jni_sig!("(I)Landroid/view/Surface;"),
                        &[JValue::Int(i32::try_from(texture_id).unwrap_or(i32::MAX))],
                    )?
                    .check_null()?
                    .l()?;
                // SAFETY: the local object is an android.view.Surface and both
                // JNI pointers remain valid for this call.
                let window = unsafe {
                    ndk::native_window::NativeWindow::from_surface(
                        env.get_raw().cast(),
                        local_surface.as_raw().cast(),
                    )
                }
                .ok_or(jni::errors::Error::NullPtr(
                    "ANativeWindow_fromSurface returned null",
                ))?;
                Ok((env.new_global_ref(local_surface)?, window))
            })
            .map_err(|error: jni::errors::Error| {
                format!("create Android video SurfaceTexture failed: {error}")
            })?;
        Ok(Self {
            app,
            vm,
            surface,
            window,
        })
    }

    pub(crate) fn native_window(&self) -> ndk::native_window::NativeWindow {
        self.window.clone()
    }

    pub(crate) fn set_buffer_size(&self, width: u32, height: u32) -> Result<(), String> {
        self.with_activity(|env, activity| {
            env.call_method(
                activity,
                jni_str!("setVideoBufferSize"),
                jni_sig!("(II)V"),
                &[
                    JValue::Int(i32::try_from(width).unwrap_or(i32::MAX)),
                    JValue::Int(i32::try_from(height).unwrap_or(i32::MAX)),
                ],
            )?;
            Ok(())
        })
    }

    pub(crate) fn update_texture(&self) -> Result<[f32; 16], String> {
        self.with_activity(|env, activity| {
            let array = env
                .call_method(
                    activity,
                    jni_str!("updateVideoTexture"),
                    jni_sig!("()[F"),
                    &[],
                )?
                .check_null()?
                .l()?;
            let array = env.cast_local::<JFloatArray>(array)?;
            let mut transform = [0.0_f32; 16];
            array.get_region(env, 0, &mut transform)?;
            Ok(transform)
        })
    }

    fn with_activity<T>(
        &self,
        callback: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        self.vm
            .attach_current_thread(|env| {
                let raw_activity = self.app.activity_as_ptr() as jni::sys::jobject;
                // SAFETY: self.app retains the NativeActivity global reference.
                let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
                callback(env, activity.as_ref())
            })
            .map_err(|error| format!("Android video surface call failed: {error}"))
    }
}

impl Drop for AndroidVideoSurface {
    fn drop(&mut self) {
        let _ = self.with_activity(|env, activity| {
            env.call_method(
                activity,
                jni_str!("releaseVideoSurface"),
                jni_sig!("()V"),
                &[],
            )?;
            Ok(())
        });
        // Keep the Java Surface global alive until after releaseVideoSurface.
        let _ = &self.surface;
    }
}

pub(crate) fn install(app: AndroidApp) {
    *ANDROID_APP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("Android app mutex poisoned") = Some(app);
}

/// Apply an Android/Linux scheduling priority to the calling native thread.
pub(crate) fn set_current_thread_priority(priority: i32) -> Result<(), String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let process = env.find_class(jni_str!("android/os/Process"))?;
        env.call_static_method(
            &process,
            jni_str!("setThreadPriority"),
            jni_sig!("(I)V"),
            &[JValue::Int(priority)],
        )?;
        Ok(())
    })
    .map_err(|error: jni::errors::Error| format!("set Android thread priority failed: {error}"))
}

/// Return whether Android appears to be running in the SDK emulator.
pub(crate) fn is_probably_emulator() -> Result<bool, String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns this activity global reference.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(
            &activity,
            jni_str!("isProbablyEmulator"),
            jni_sig!("()Z"),
            &[],
        )?
        .z()
    })
    .map_err(|error: jni::errors::Error| format!("detect Android emulator failed: {error}"))
}

pub(crate) fn open_key_file(context: eframe::egui::Context) -> Result<(), String> {
    *KEY_FILE_CONTEXT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("Android key picker context mutex poisoned") = Some(context);
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns this global activity reference for the
        // lifetime of the cloned AndroidApp handle.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(&activity, jni_str!("openKeyFile"), jni_sig!("()V"), &[])?;
        Ok(())
    })
    .map_err(|error: jni::errors::Error| format!("Android key picker failed: {error}"))
}

pub(crate) fn save_file(name: &str, bytes: &[u8]) -> Result<(), String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns this activity global reference.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        let name = env.new_string(name)?;
        let bytes = env.byte_array_from_slice(bytes)?;
        env.call_method(
            &activity,
            jni_str!("saveSupportBundle"),
            jni_sig!("(Ljava/lang/String;[B)V"),
            &[
                JValue::Object(name.as_ref()),
                JValue::Object(bytes.as_ref()),
            ],
        )?;
        Ok(())
    })
    .map_err(|error: jni::errors::Error| format!("Android document export failed: {error}"))
}

/// App-owned recording directory used without opening Android's document picker.
pub(crate) fn recordings_directory() -> Result<std::path::PathBuf, String> {
    app()?
        .internal_data_path()
        .map(|root| root.join("nebulus").join("recordings"))
        .ok_or_else(|| "Android internal storage is unavailable".to_owned())
}

pub(crate) fn take_key_file_result() -> Option<Result<SelectedKeyFile, String>> {
    KEY_FILE_RESULT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()?
        .take()
}

pub(crate) fn open_vpn() -> Result<OpenedVpn, String> {
    let state = VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
    let app = app()?;
    let vm = java_vm(&app)?;
    {
        let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
        if request.waiting {
            return Err("Android VPN permission request is already active".to_owned());
        }
        request.waiting = true;
        request.result = None;
    }
    let request_result: Result<(), jni::errors::Error> = vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns the activity for the lifetime of `app`.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(&activity, jni_str!("openVpn"), jni_sig!("()V"), &[])?;
        Ok(())
    });
    if let Err(error) = request_result {
        state
            .0
            .lock()
            .expect("Android VPN result mutex poisoned")
            .waiting = false;
        return Err(format!("Android VPN request failed: {error}"));
    }

    let mut request = state
        .1
        .wait_timeout_while(
            state.0.lock().expect("Android VPN result mutex poisoned"),
            USB_PERMISSION_TIMEOUT,
            |request| request.result.is_none(),
        )
        .map_err(|_| "Android VPN result mutex poisoned".to_owned())?
        .0;
    request.waiting = false;
    let result = request.result.take();
    result.unwrap_or_else(|| Err("Android VPN permission timed out".to_owned()))
}

pub(crate) fn close_vpn(fd: i32) {
    let Ok(app) = app() else {
        return;
    };
    let Ok(vm) = java_vm(&app) else {
        return;
    };
    let _: Result<(), jni::errors::Error> = vm.attach_current_thread(|env| {
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns the activity for the lifetime of `app`.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(
            &activity,
            jni_str!("closeVpn"),
            jni_sig!("(I)V"),
            &[JValue::Int(fd)],
        )?;
        Ok(())
    });
}

fn finish_key_file(result: Result<SelectedKeyFile, String>) {
    if let Ok(mut pending) = KEY_FILE_RESULT.get_or_init(|| Mutex::new(None)).lock() {
        *pending = Some(result);
    }
    if let Some(context) = KEY_FILE_CONTEXT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut context| context.take())
    {
        context.request_repaint();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeKeySelected<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
    bytes: JByteArray<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            finish_key_file(Ok(SelectedKeyFile {
                name: name.try_to_string(env)?,
                bytes: env.convert_byte_array(&bytes)?,
            }));
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeKeyError<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    message: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            finish_key_file(Err(message.try_to_string(env)?));
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeVpnOpened<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    fd: i32,
    interface_name: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let name = interface_name.try_to_string(env)?;
            let state =
                VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
            let accepted = {
                let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
                if request.waiting {
                    request.result = Some(Ok(OpenedVpn {
                        fd,
                        interface_name: name,
                    }));
                    true
                } else {
                    false
                }
            };
            if accepted {
                state.1.notify_all();
            } else {
                close_vpn(fd);
            }
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_neels_openipc_nebulus_NebulusActivity_nativeVpnError<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    message: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let message = message.try_to_string(env)?;
            let state =
                VPN_RESULT.get_or_init(|| (Mutex::new(VpnRequestState::default()), Condvar::new()));
            let mut request = state.0.lock().expect("Android VPN result mutex poisoned");
            if request.waiting {
                request.result = Some(Err(message));
                state.1.notify_all();
            }
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

fn app() -> Result<AndroidApp, String> {
    ANDROID_APP
        .get()
        .and_then(|app| app.lock().ok()?.clone())
        .ok_or_else(|| "Android activity is not ready".to_owned())
}

pub(crate) fn list_devices() -> Result<Vec<UsbDeviceInfo>, String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    vm.attach_current_thread(|env| {
        let manager = usb_manager(env, &app)?;
        let iterator = device_iterator(env, &manager)?;
        let mut devices = Vec::new();
        while env
            .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
            .z()?
        {
            let device = env
                .call_method(
                    &iterator,
                    jni_str!("next"),
                    jni_sig!("()Ljava/lang/Object;"),
                    &[],
                )?
                .l()?;
            if let Some(info) = device_info(env, &device)? {
                devices.push(info);
            }
        }
        Ok(devices)
    })
    .map_err(|error: jni::errors::Error| format!("Android USB discovery failed: {error}"))
}

/// Ask Android for access when needed and convert its connection fd to nusb.
pub(crate) fn open_device(selected: Option<&str>) -> Result<OpenedUsbDevice, String> {
    let app = app()?;
    let vm = java_vm(&app)?;
    let selected = selected.map(str::to_owned);
    let (manager, device, info, permission_granted) = vm
        .attach_current_thread(|env| {
            let manager = usb_manager(env, &app)?;
            let iterator = device_iterator(env, &manager)?;
            let mut found = None;
            while env
                .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
                .z()?
            {
                let device = env
                    .call_method(
                        &iterator,
                        jni_str!("next"),
                        jni_sig!("()Ljava/lang/Object;"),
                        &[],
                    )?
                    .l()?;
                let Some(info) = device_info(env, &device)? else {
                    continue;
                };
                let matches = selected
                    .as_deref()
                    .map(|id| info.id == id || info.id.starts_with(id))
                    .unwrap_or(true);
                if matches {
                    found = Some((device, info));
                    break;
                }
            }
            let (device, info) = found.ok_or_else(|| {
                jni::errors::Error::NullPtr("No supported Realtek USB adapter is attached")
            })?;
            let granted = has_permission(env, &manager, &device)?;
            if !granted {
                request_permission(env, &app, &manager, &device)?;
            }
            Ok((
                env.new_global_ref(manager)?,
                env.new_global_ref(device)?,
                info,
                granted,
            ))
        })
        .map_err(|error: jni::errors::Error| format!("Android USB open failed: {error}"))?;

    if !permission_granted {
        let started = Instant::now();
        loop {
            let granted = vm
                .attach_current_thread(|env| has_permission(env, &manager, &device))
                .map_err(|error: jni::errors::Error| {
                    format!("Android USB permission check failed: {error}")
                })?;
            if granted {
                break;
            }
            if started.elapsed() >= USB_PERMISSION_TIMEOUT {
                return Err("Android USB permission was not granted".to_owned());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let (fd, connection) = vm
        .attach_current_thread(|env| {
            let connection = env
                .call_method(
                    &manager,
                    jni_str!("openDevice"),
                    jni_sig!("(Landroid/hardware/usb/UsbDevice;)Landroid/hardware/usb/UsbDeviceConnection;"),
                    &[JValue::Object(device.as_ref())],
                )?
                .check_null()?
                .l()?;
            let fd = env
                .call_method(
                    &connection,
                    jni_str!("getFileDescriptor"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            Ok((fd, env.new_global_ref(connection)?))
        })
        .map_err(|error: jni::errors::Error| format!("Android USB open failed: {error}"))?;
    let owned_fd = duplicate_fd(fd)?;
    let device = nusb::Device::from_fd(owned_fd)
        .wait()
        .map_err(|error| format!("nusb could not open Android USB fd: {error}"))?;
    Ok(OpenedUsbDevice {
        device,
        info,
        connection,
    })
}

fn java_vm(app: &AndroidApp) -> Result<JavaVM, String> {
    // SAFETY: AndroidActivity owns this VM for at least as long as its cloned
    // AndroidApp handle. JavaVM is a non-owning process-wide handle.
    Ok(unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) })
}

fn usb_manager<'local>(
    env: &mut jni::Env<'local>,
    app: &AndroidApp,
) -> jni::errors::Result<JObject<'local>> {
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    // SAFETY: android-activity documents this pointer as an unowned global
    // reference valid for the lifetime of `app`; Cast does not delete it.
    let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
    let service = env.new_string("usb")?;
    env.call_method(
        &activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(service.as_ref())],
    )?
    .check_null()?
    .l()
}

fn device_iterator<'local>(
    env: &mut jni::Env<'local>,
    manager: &JObject<'local>,
) -> jni::errors::Result<JObject<'local>> {
    let map = env
        .call_method(
            manager,
            jni_str!("getDeviceList"),
            jni_sig!("()Ljava/util/HashMap;"),
            &[],
        )?
        .check_null()?
        .l()?;
    let values = env
        .call_method(
            &map,
            jni_str!("values"),
            jni_sig!("()Ljava/util/Collection;"),
            &[],
        )?
        .check_null()?
        .l()?;
    env.call_method(
        &values,
        jni_str!("iterator"),
        jni_sig!("()Ljava/util/Iterator;"),
        &[],
    )?
    .check_null()?
    .l()
}

fn device_info(
    env: &mut jni::Env<'_>,
    device: &JObject<'_>,
) -> jni::errors::Result<Option<UsbDeviceInfo>> {
    let vendor_id = env
        .call_method(device, jni_str!("getVendorId"), jni_sig!("()I"), &[])?
        .i()? as u16;
    let product_id = env
        .call_method(device, jni_str!("getProductId"), jni_sig!("()I"), &[])?
        .i()? as u16;
    let Some(supported) = openipc_rtl88xx::supported_device(vendor_id, product_id) else {
        return Ok(None);
    };
    let device_name = env
        .call_method(
            device,
            jni_str!("getDeviceName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .check_null()?
        .l()?;
    let device_name = env.cast_local::<JString>(device_name)?.try_to_string(env)?;
    Ok(Some(UsbDeviceInfo {
        id: format!("{vendor_id:04x}:{product_id:04x}@android-{device_name}"),
        label: supported.label.to_owned(),
        vendor_id,
        product_id,
        location: device_name,
    }))
}

fn has_permission<'local, 'object>(
    env: &mut jni::Env<'local>,
    manager: &impl AsRef<JObject<'object>>,
    device: &impl AsRef<JObject<'object>>,
) -> jni::errors::Result<bool> {
    env.call_method(
        manager,
        jni_str!("hasPermission"),
        jni_sig!("(Landroid/hardware/usb/UsbDevice;)Z"),
        &[JValue::Object(device.as_ref())],
    )?
    .z()
}

fn request_permission(
    env: &mut jni::Env<'_>,
    app: &AndroidApp,
    manager: &JObject<'_>,
    device: &JObject<'_>,
) -> jni::errors::Result<()> {
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    // SAFETY: See `usb_manager`; this is another scoped borrow of the same
    // android-activity-owned global reference.
    let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
    let action = env.new_string("dev.neels.openipc.nebulus.USB_PERMISSION")?;
    let intent_class = env.find_class(jni_str!("android/content/Intent"))?;
    let intent = env.new_object(
        &intent_class,
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(action.as_ref())],
    )?;
    let pending_class = env.find_class(jni_str!("android/app/PendingIntent"))?;
    const FLAG_UPDATE_CURRENT: i32 = 0x0800_0000;
    const FLAG_MUTABLE: i32 = 0x0200_0000;
    let pending = env
        .call_static_method(
            &pending_class,
            jni_str!("getBroadcast"),
            jni_sig!(
                "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
            ),
            &[
                JValue::Object(activity.as_ref()),
                JValue::Int(0),
                JValue::Object(&intent),
                JValue::Int(FLAG_UPDATE_CURRENT | FLAG_MUTABLE),
            ],
        )?
        .check_null()?
        .l()?;
    env.call_method(
        manager,
        jni_str!("requestPermission"),
        jni_sig!("(Landroid/hardware/usb/UsbDevice;Landroid/app/PendingIntent;)V"),
        &[JValue::Object(device), JValue::Object(&pending)],
    )?;
    Ok(())
}

fn duplicate_fd(fd: i32) -> Result<OwnedFd, String> {
    if fd < 0 {
        return Err(format!("Android returned invalid USB fd {fd}"));
    }
    // SAFETY: `fd` comes from a live UsbDeviceConnection. `dup` creates an
    // independent descriptor that can be transferred to nusb.
    let duplicate = unsafe { libc::dup(fd) };
    if duplicate < 0 {
        return Err(format!(
            "duplicate Android USB fd failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `dup` returned a fresh descriptor now exclusively owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}
