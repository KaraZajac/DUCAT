use super::*;
use crate::veilid_api::android::ANDROID_GLOBALS;
use jni::errors::Result as JniResult;
use jni::objects::JString;
use jni::{jni_sig, jni_str};

#[allow(dead_code)]
pub fn get_files_dir() -> String {
    let aglock = ANDROID_GLOBALS.lock();
    let ag = aglock.as_ref().unwrap_or_log();

    ag.vm
        .attach_current_thread(|env| {
            env.with_local_frame(64, |env| {
                // context.getFilesDir().getAbsolutePath()
                let file = env
                    .call_method(
                        ag.ctx.as_obj(),
                        jni_str!("getFilesDir"),
                        jni_sig!("()Ljava/io/File;"),
                        &[],
                    )
                    .unwrap_or_log()
                    .l()
                    .unwrap_or_log();
                let path = env
                    .call_method(
                        file,
                        jni_str!("getAbsolutePath"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )
                    .unwrap_or_log()
                    .l()
                    .unwrap_or_log();

                let jstr = env.cast_local::<JString>(path).unwrap_or_log();
                JniResult::Ok(jstr.try_to_string(env).unwrap_or_log())
            })
        })
        .unwrap_or_log()
}

// XXX: android doesn't allow creating directories in the cache directory
// so we need to create them in the files directory
// #[allow(dead_code)]
// pub fn get_cache_dir() -> String {
//     let aglock = ANDROID_GLOBALS.lock();
//     let ag = aglock.as_ref().unwrap_or_log();
//     let mut env = ag.vm.attach_current_thread().unwrap_or_log();

//     env.with_local_frame(64, |env| {
//         // context.getCacheDir().getAbsolutePath()
//         let file = env
//             .call_method(ag.ctx.as_obj(), "getCacheDir", "()Ljava/io/File;", &[])
//             .unwrap_or_log()
//             .l()
//             .unwrap_or_log();
//         let path = env
//             .call_method(file, "getAbsolutePath", "()Ljava/lang/String;", &[])
//             .unwrap_or_log()
//             .l()
//             .unwrap_or_log();

//         let jstr = JString::from(path);
//         let jstrval = env.get_string(&jstr).unwrap_or_log();
//         JniResult::Ok(String::from(jstrval.to_string_lossy()))
//     })
//     .unwrap_or_log()
// }
