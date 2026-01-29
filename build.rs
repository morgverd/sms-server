fn main() {
    feature_conflicts();

    let version = get_version();
    println!("cargo:rustc-env=VERSION={version}");
    println!("cargo:warning=Feature tagged version: {version}");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
}

fn feature_conflicts() {
    // TLS
    let tls_rustls = std::env::var("CARGO_FEATURE_TLS_RUSTLS").is_ok();
    let tls_native = std::env::var("CARGO_FEATURE_TLS_NATIVE").is_ok();

    if tls_rustls && tls_native {
        panic!(
            "Cannot enable both 'tls-rustls' and 'tls-native' features simultaneously. Choose one."
        );
    }
    if !tls_rustls && !tls_native {
        println!("cargo:warning=No TLS backend selected. Consider enabling either 'tls-rustls' or 'tls-native' features for production use!");
    }

    // Sentry
    let sentry = std::env::var("CARGO_FEATURE_SENTRY").is_ok();
    if sentry && !tls_rustls && !tls_native {
        panic!("The 'sentry' feature requires at least one TLS backend. Enable either 'tls-rustls' or 'tls-native' feature");
    }

    // Database
    let db_sqlite = std::env::var("CARGO_FEATURE_DB_SQLITE").is_ok();
    if !db_sqlite {
        panic!("At least one database backend feature must be enabled!");
    }
}

/// Creates a version string from the package version, with all
/// optional features included in the build metadata suffix.
fn get_version() -> String {
    let mut suffixes = Vec::new();
    let feature_names = vec![
        ("GPIO", "g"),
        ("OPENAPI", "o"),
        ("HTTP_SERVER", "h"),
        ("SENTRY", "s"),
        ("TLS_NATIVE", "tn"),
        ("TLS_RUSTLS", "tr"),
    ];
    for (feature, name) in feature_names {
        if std::env::var(format!("CARGO_FEATURE_{feature}")).is_ok() {
            suffixes.push(name);
        }
    }

    let version = env!("CARGO_PKG_VERSION");
    let full_version = if suffixes.is_empty() {
        version.to_string()
    } else {
        format!("{}+{}", version, suffixes.join(""))
    };

    full_version
}
