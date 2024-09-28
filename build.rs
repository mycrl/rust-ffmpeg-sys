use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use bindgen::callbacks::{
    EnumVariantCustomBehavior, EnumVariantValue, IntKind, MacroParsingBehavior, ParseCallbacks,
};

fn join(root: &str, next: &str) -> anyhow::Result<String> {
    Ok(Path::new(root)
        .join(next)
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to path into string."))?
        .to_string())
}

fn is_exsit(dir: &str) -> bool {
    fs::metadata(dir).is_ok()
}

fn exec(command: &str, work_dir: &str) -> anyhow::Result<String> {
    let output = Command::new(if cfg!(target_os = "windows") {
        "powershell"
    } else {
        "bash"
    })
    .arg(if cfg!(target_os = "windows") {
        "-command"
    } else {
        "-c"
    })
    .arg(if cfg!(target_os = "windows") {
        format!("$ProgressPreference = 'SilentlyContinue';{}", command)
    } else {
        command.to_string()
    })
    .current_dir(work_dir)
    .output()?;
    if !output.status.success() {
        Err(anyhow::anyhow!("{}", unsafe {
            String::from_utf8_unchecked(output.stderr)
        }))
    } else {
        Ok(unsafe { String::from_utf8_unchecked(output.stdout) })
    }
}

#[derive(Debug)]
struct Callbacks;

impl ParseCallbacks for Callbacks {
    fn int_macro(&self, _name: &str, value: i64) -> Option<IntKind> {
        let ch_layout_prefix = "AV_CH_";
        let codec_cap_prefix = "AV_CODEC_CAP_";
        let codec_flag_prefix = "AV_CODEC_FLAG_";
        let error_max_size = "AV_ERROR_MAX_STRING_SIZE";

        if _name.starts_with(ch_layout_prefix) {
            Some(IntKind::ULongLong)
        } else if value >= i32::MIN as i64
            && value <= i32::MAX as i64
            && (_name.starts_with(codec_cap_prefix) || _name.starts_with(codec_flag_prefix))
        {
            Some(IntKind::UInt)
        } else if _name == error_max_size {
            Some(IntKind::Custom {
                name: "usize",
                is_signed: false,
            })
        } else if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
            Some(IntKind::Int)
        } else {
            None
        }
    }

    fn enum_variant_behavior(
        &self,
        _enum_name: Option<&str>,
        original_variant_name: &str,
        _variant_value: EnumVariantValue,
    ) -> Option<EnumVariantCustomBehavior> {
        let dummy_codec_id_prefix = "AV_CODEC_ID_FIRST_";
        if original_variant_name.starts_with(dummy_codec_id_prefix) {
            Some(EnumVariantCustomBehavior::Constify)
        } else {
            None
        }
    }

    // https://github.com/rust-lang/rust-bindgen/issues/687#issuecomment-388277405
    fn will_parse_macro(&self, name: &str) -> MacroParsingBehavior {
        use MacroParsingBehavior::*;

        match name {
            "FP_INFINITE" => Ignore,
            "FP_NAN" => Ignore,
            "FP_NORMAL" => Ignore,
            "FP_SUBNORMAL" => Ignore,
            "FP_ZERO" => Ignore,
            _ => Default,
        }
    }
}

fn output() -> PathBuf {
    PathBuf::from(env::var("OUT_DIR").unwrap())
}

fn search_include(include_prefix: &Vec<String>, header: &str) -> String {
    for dir in include_prefix {
        let include = join(dir, header).unwrap();
        if fs::metadata(&include).is_ok() {
            return include;
        }
    }
    format!("/usr/include/{}", header)
}

static LIBRARYS: [(&str, &str); 8] = [
    ("avcodec", "6.0"),
    ("avdevice", "6.0"),
    ("avfilter", "6.0"),
    ("avformat", "6.0"),
    ("avutil", "6.0"),
    ("postproc", "6.0"),
    ("swresample", "4.7"),
    ("swscale", "6.0"),
];

fn main() -> anyhow::Result<()> {
    let out_dir = env::var("OUT_DIR")?;
    let is_debug = env::var("DEBUG")
        .map(|label| label == "true")
        .unwrap_or(true);

    let (mut include_prefix, lib_prefix) = find_ffmpeg_prefix(&out_dir, is_debug)?;
    for path in &lib_prefix {
        println!("cargo:rustc-link-search=all={}", path);
    }

    for (lib, _) in LIBRARYS {
        println!("cargo:rustc-link-lib={}", lib);
    }

    if cfg!(target_os = "macos") {
        for f in [
            "AppKit",
            "AudioToolbox",
            "AVFoundation",
            "CoreFoundation",
            "CoreGraphics",
            "CoreMedia",
            "CoreServices",
            "CoreVideo",
            "Foundation",
            "OpenCL",
            "OpenGL",
            "QTKit",
            "QuartzCore",
            "Security",
            "VideoDecodeAcceleration",
            "VideoToolbox",
        ] {
            println!("cargo:rustc-link-lib=framework={}", f);
        }
    }

    let media_sdk_prefix = join(&out_dir, "media-sdk").unwrap();
    if !is_exsit(&media_sdk_prefix) {
        exec(
            "git clone https://github.com/Intel-Media-SDK/MediaSDK media-sdk",
            &out_dir,
        )?;
    }

    let media_sdk_include_prefix = join(&media_sdk_prefix, "./api/include")?;
    include_prefix.append(&mut vec![media_sdk_include_prefix.clone()]);

    let clang_includes = include_prefix
        .iter()
        .map(|include| format!("-I{}", include));

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let mut builder = bindgen::Builder::default()
        .clang_args(clang_includes)
        .ctypes_prefix("libc")
        // https://github.com/rust-lang/rust-bindgen/issues/550
        .blocklist_type("max_align_t")
        .blocklist_function("_.*")
        // Blocklist functions with u128 in signature.
        // https://github.com/zmwangx/rust-ffmpeg-sys/issues/1
        // https://github.com/rust-lang/rust-bindgen/issues/1549
        .blocklist_function("acoshl")
        .blocklist_function("acosl")
        .blocklist_function("asinhl")
        .blocklist_function("asinl")
        .blocklist_function("atan2l")
        .blocklist_function("atanhl")
        .blocklist_function("atanl")
        .blocklist_function("cbrtl")
        .blocklist_function("ceill")
        .blocklist_function("copysignl")
        .blocklist_function("coshl")
        .blocklist_function("cosl")
        .blocklist_function("dreml")
        .blocklist_function("ecvt_r")
        .blocklist_function("erfcl")
        .blocklist_function("erfl")
        .blocklist_function("exp2l")
        .blocklist_function("expl")
        .blocklist_function("expm1l")
        .blocklist_function("fabsl")
        .blocklist_function("fcvt_r")
        .blocklist_function("fdiml")
        .blocklist_function("finitel")
        .blocklist_function("floorl")
        .blocklist_function("fmal")
        .blocklist_function("fmaxl")
        .blocklist_function("fminl")
        .blocklist_function("fmodl")
        .blocklist_function("frexpl")
        .blocklist_function("gammal")
        .blocklist_function("hypotl")
        .blocklist_function("ilogbl")
        .blocklist_function("isinfl")
        .blocklist_function("isnanl")
        .blocklist_function("j0l")
        .blocklist_function("j1l")
        .blocklist_function("jnl")
        .blocklist_function("ldexpl")
        .blocklist_function("lgammal")
        .blocklist_function("lgammal_r")
        .blocklist_function("llrintl")
        .blocklist_function("llroundl")
        .blocklist_function("log10l")
        .blocklist_function("log1pl")
        .blocklist_function("log2l")
        .blocklist_function("logbl")
        .blocklist_function("logl")
        .blocklist_function("lrintl")
        .blocklist_function("lroundl")
        .blocklist_function("modfl")
        .blocklist_function("nanl")
        .blocklist_function("nearbyintl")
        .blocklist_function("nextafterl")
        .blocklist_function("nexttoward")
        .blocklist_function("nexttowardf")
        .blocklist_function("nexttowardl")
        .blocklist_function("powl")
        .blocklist_function("qecvt")
        .blocklist_function("qecvt_r")
        .blocklist_function("qfcvt")
        .blocklist_function("qfcvt_r")
        .blocklist_function("qgcvt")
        .blocklist_function("remainderl")
        .blocklist_function("remquol")
        .blocklist_function("rintl")
        .blocklist_function("roundl")
        .blocklist_function("scalbl")
        .blocklist_function("scalblnl")
        .blocklist_function("scalbnl")
        .blocklist_function("significandl")
        .blocklist_function("sinhl")
        .blocklist_function("sinl")
        .blocklist_function("sqrtl")
        .blocklist_function("strtold")
        .blocklist_function("tanhl")
        .blocklist_function("tanl")
        .blocklist_function("tgammal")
        .blocklist_function("truncl")
        .blocklist_function("y0l")
        .blocklist_function("y1l")
        .blocklist_function("ynl")
        .opaque_type("__mingw_ldbl_type_t")
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: false,
        })
        .prepend_enum_name(false)
        .derive_eq(true)
        .size_t_is_usize(true)
        .parse_callbacks(Box::new(Callbacks))
        .header(search_include(&include_prefix, "libavcodec/avcodec.h"))
        .header(search_include(&include_prefix, "libavcodec/dv_profile.h"))
        .header(search_include(&include_prefix, "libavcodec/avfft.h"))
        .header(search_include(
            &include_prefix,
            "libavcodec/vorbis_parser.h",
        ))
        .header(search_include(&include_prefix, "libavdevice/avdevice.h"))
        .header(search_include(&include_prefix, "libavfilter/buffersink.h"))
        .header(search_include(&include_prefix, "libavfilter/buffersrc.h"))
        .header(search_include(&include_prefix, "libavfilter/avfilter.h"))
        .header(search_include(&include_prefix, "libavformat/avformat.h"))
        .header(search_include(&include_prefix, "libavformat/avio.h"))
        .header(search_include(&include_prefix, "libavutil/adler32.h"))
        .header(search_include(&include_prefix, "libavutil/aes.h"))
        .header(search_include(&include_prefix, "libavutil/audio_fifo.h"))
        .header(search_include(&include_prefix, "libavutil/base64.h"))
        .header(search_include(&include_prefix, "libavutil/blowfish.h"))
        .header(search_include(&include_prefix, "libavutil/bprint.h"))
        .header(search_include(&include_prefix, "libavutil/buffer.h"))
        .header(search_include(&include_prefix, "libavutil/camellia.h"))
        .header(search_include(&include_prefix, "libavutil/cast5.h"))
        .header(search_include(
            &include_prefix,
            "libavutil/channel_layout.h",
        ))
        // Here until https://github.com/rust-lang/rust-bindgen/issues/2192 /
        // https://github.com/rust-lang/rust-bindgen/issues/258 is fixed.
        .header("channel_layout_fixed.h")
        .header(search_include(&include_prefix, "libavutil/cpu.h"))
        .header(search_include(&include_prefix, "libavutil/crc.h"))
        .header(search_include(&include_prefix, "libavutil/dict.h"))
        .header(search_include(&include_prefix, "libavutil/display.h"))
        .header(search_include(&include_prefix, "libavutil/downmix_info.h"))
        .header(search_include(&include_prefix, "libavutil/error.h"))
        .header(search_include(&include_prefix, "libavutil/eval.h"))
        .header(search_include(&include_prefix, "libavutil/fifo.h"))
        .header(search_include(&include_prefix, "libavutil/file.h"))
        .header(search_include(&include_prefix, "libavutil/frame.h"))
        .header(search_include(&include_prefix, "libavutil/hash.h"))
        .header(search_include(&include_prefix, "libavutil/hmac.h"))
        .header(search_include(&include_prefix, "libavutil/hwcontext.h"))
        .header(search_include(&include_prefix, "libavutil/imgutils.h"))
        .header(search_include(&include_prefix, "libavutil/lfg.h"))
        .header(search_include(&include_prefix, "libavutil/log.h"))
        .header(search_include(&include_prefix, "libavutil/lzo.h"))
        .header(search_include(&include_prefix, "libavutil/macros.h"))
        .header(search_include(&include_prefix, "libavutil/mathematics.h"))
        .header(search_include(&include_prefix, "libavutil/md5.h"))
        .header(search_include(&include_prefix, "libavutil/mem.h"))
        .header(search_include(&include_prefix, "libavutil/motion_vector.h"))
        .header(search_include(&include_prefix, "libavutil/murmur3.h"))
        .header(search_include(&include_prefix, "libavutil/opt.h"))
        .header(search_include(&include_prefix, "libavutil/parseutils.h"))
        .header(search_include(&include_prefix, "libavutil/pixdesc.h"))
        .header(search_include(&include_prefix, "libavutil/pixfmt.h"))
        .header(search_include(&include_prefix, "libavutil/random_seed.h"))
        .header(search_include(&include_prefix, "libavutil/rational.h"))
        .header(search_include(&include_prefix, "libavutil/replaygain.h"))
        .header(search_include(&include_prefix, "libavutil/ripemd.h"))
        .header(search_include(&include_prefix, "libavutil/samplefmt.h"))
        .header(search_include(&include_prefix, "libavutil/sha.h"))
        .header(search_include(&include_prefix, "libavutil/sha512.h"))
        .header(search_include(&include_prefix, "libavutil/stereo3d.h"))
        .header(search_include(&include_prefix, "libavutil/avstring.h"))
        .header(search_include(&include_prefix, "libavutil/threadmessage.h"))
        .header(search_include(&include_prefix, "libavutil/time.h"))
        .header(search_include(&include_prefix, "libavutil/timecode.h"))
        .header(search_include(&include_prefix, "libavutil/twofish.h"))
        .header(search_include(&include_prefix, "libavutil/avutil.h"))
        .header(search_include(&include_prefix, "libavutil/xtea.h"))
        .header(search_include(&include_prefix, "libpostproc/postprocess.h"))
        .header(search_include(
            &include_prefix,
            "libswresample/swresample.h",
        ))
        .header(search_include(&include_prefix, "libpostproc/postprocess.h"));

    #[cfg(target_os = "windows")]
    {
        builder = builder
            .header(search_include(&include_prefix, "libavutil/hwcontext_qsv.h"))
            .header(search_include(
                &include_prefix,
                "libavutil/hwcontext_d3d11va.h",
            ));
    }

    #[cfg(target_os = "linux")]
    {
        builder = builder.header(search_include(&include_prefix, "libavutil/hwcontext_drm.h"));
    }

    // Finish the builder and generate the bindings.
    let bindings = builder
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    bindings
        .write_to_file(output().join("bindings.rs"))
        .expect("Couldn't write bindings!");

    Ok(())
}

fn find_ffmpeg_prefix(out_dir: &str, is_debug: bool) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    if cfg!(target_os = "macos") {
        let prefix = exec("brew --prefix ffmpeg@6", out_dir)?.replace('\n', "");

        Ok((
            vec![join(&prefix, "./include")?],
            vec![join(&prefix, "./lib")?],
        ))
    } else if cfg!(target_os = "windows") {
        let prefix = join(out_dir, "ffmpeg").unwrap();
        if !is_exsit(&prefix) {
            exec(
                    &format!(
                        "Invoke-WebRequest -Uri https://github.com/mycrl/third-party/releases/download/distributions/ffmpeg-windows-x64-{}.zip -OutFile ffmpeg.zip", 
                        if is_debug { "debug" } else { "release" }
                    ),
                    out_dir,
                )?;

            exec(
                "Expand-Archive -Path ffmpeg.zip -DestinationPath ./",
                out_dir,
            )?;
        }

        Ok((
            vec![join(&prefix, "./include")?],
            vec![join(&prefix, "./lib")?],
        ))
    } else {
        let mut librarys = Vec::new();
        let mut includes = Vec::new();

        for (name, version) in LIBRARYS {
            let lib = pkg_config::Config::new()
                .atleast_version(version)
                .probe(&format!("lib{}", name))?;

            for path in lib.link_paths {
                librarys.push(path.to_str().unwrap().to_string());
            }

            for path in lib.include_paths {
                includes.push(path.to_str().unwrap().to_string());
            }
        }

        Ok((includes, librarys))
    }
}
