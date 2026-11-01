//! Socket naming for applications (FR-001, FR-002). One test function, because it changes the
//! process environment.

use rubickway::server::Server;

#[test]
fn listens_on_the_first_free_name_and_cleans_up() {
    let dir = std::env::temp_dir().join(format!("rubickway-socket-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    set_runtime_dir(&dir);

    let (first, first_name) = Server::listen().unwrap();
    assert_eq!(first_name, "wayland-1");
    let (second, second_name) = Server::listen().unwrap();
    assert_eq!(second_name, "wayland-2");
    for name in ["wayland-1", "wayland-1.lock", "wayland-2", "wayland-2.lock"] {
        assert!(dir.join(name).exists(), "{name} missing");
    }

    drop(first);
    assert!(!dir.join("wayland-1").exists());
    assert!(!dir.join("wayland-1.lock").exists());
    drop(second);
    assert!(!dir.join("wayland-2").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Points `XDG_RUNTIME_DIR` at `dir` for this process.
fn set_runtime_dir(dir: &std::path::Path) {
    // `set_var` is unsafe in edition 2024 because other threads may read the environment;
    // this binary runs one test on one thread before anything else starts.
    #[expect(
        unsafe_code,
        reason = "single-threaded test setup changes the environment"
    )]
    // SAFETY: no other thread exists yet in this single-test binary.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir);
    }
}
