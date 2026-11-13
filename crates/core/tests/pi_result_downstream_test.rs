//! 真实独立工程（independent projects）回归兼容错误派生（compatible error derive）。
//! 每个目标（target）只依赖pi_result或桥接库（bridge crate），不额外添加thiserror。
//! 编译与运行（compile/run）串行执行，每个子进程（child process）最多180秒；
//! 拒绝编译（compile-fail）须命中目标诊断（target diagnostic），依赖故障（dependency failure）不算通过。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// 测试目标目录（test target directory）与父Cargo不同，避免嵌套构建锁（nested build lock）。
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("pi-result-downstream")
}

/// 读取两个输出流（output streams）避免管道填满（pipe saturation），截止后回收子进程（reap child process）。
fn cargo(project: &str, action: &str, bin: &str) -> Output {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures").join(project).join("Cargo.toml");
    let mut child = Command::new(env!("CARGO"))
        .args([action, "--manifest-path"]).arg(manifest)
        .args(["--locked", "--offline", "--bin", bin, "--target-dir"])
        .arg(target_dir()).env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() { break status; }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            timed_out = true;
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output { status, stdout: out.join().unwrap(), stderr: err.join().unwrap() };
    assert!(!timed_out, "downstream deadline: {project}/{bin}: {output:?}");
    output
}

/// 成功路径（success path）必须执行实际断言（real assertions）并输出唯一标记（unique marker）。
fn assert_run(project: &str, bin: &str, marker: &str) {
    let output = cargo(project, "run", bin);
    assert!(output.status.success(), "{project}/{bin}: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), marker);
}

/// 失败判据（failure oracle）同时核退出码、原生位置和明确诊断（exit code/native location/diagnostic）。
fn assert_rejected(project: &str, bin: &str, diagnostics: &[&str]) {
    let output = cargo(project, "check", bin);
    assert_eq!(output.status.code(), Some(101), "expected compile rejection: {project}/{bin}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostics.iter().all(|message| stderr.contains(message)), "{project}/{bin}: {stderr}");
    assert!(stderr.replace('\\', "/").contains(&format!("src/bin/{bin}.rs:")), "missing native span: {stderr}");
    assert!(!stderr.contains("failed to get") && !stderr.contains("failed to download"), "dependency failure: {stderr}");
    assert!(!stderr.contains("proc-macro derive panicked"), "macro panic: {stderr}");
}

/// 推荐/旧入口、泛型与借用（recommended/legacy entries/generics/borrows）由直接工程（direct project）运行；
/// 重命名及间接重导出（renaming/indirect re-export）使用各自真实清单（actual manifests）。
/// 正反对照（positive/negative controls）验证Report约束及路径配置（Report bounds/path configuration）。
// @trace 1
#[test]
fn test_runtime_only_derive_contract_and_compile_rejections() {
    assert_run("pi_result_compat", "pi-clock-error-compat", "PI_CLOCK_DERIVE_COMPAT_OK");
    assert_run("pi_result_renamed", "pi-clock-error-renamed", "PI_CLOCK_RENAMED_OK");
    assert_run("pi_result_indirect", "pi-clock-error-indirect", "PI_CLOCK_INDIRECT_OK");
    for (bin, diagnostics) in [
        ("non_send", &["E0277", "Send", "Sync"][..]),
        ("non_static", &["E0597", "'static"][..]),
        ("string_path", &["expected a plain Rust crate path"][..]),
        ("missing_path", &["E0433", "missing_runtime"][..]),
    ] {
        assert_rejected("pi_result_compat", bin, diagnostics);
    }
    assert_rejected("pi_result_indirect", "missing_override", &["dependency discovery failed"]);

    let compiler = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("--version").output().unwrap();
    assert!(compiler.status.success());
    let version = String::from_utf8(compiler.stdout).unwrap();
    if version.contains("nightly") || version.contains("-dev") {
        assert_run("pi_result_compat", "provider", "PI_CLOCK_PROVIDER_OK");
        assert_rejected("pi_result_compat", "provider_no_feature", &["E0658", "error_generic_member_access"]);
    } else {
        assert_rejected("pi_result_compat", "provider", &["E0554"]);
    }
}
