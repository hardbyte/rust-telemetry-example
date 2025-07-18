use progenitor::{GenerationSettings, Generator, InterfaceStyle};
use quote::quote;
use std::process::Command;

fn main() {
    let src = "spec.yaml";
    let file = std::fs::File::open(src).expect("Could not open openapi spec file");

    let spec = serde_yaml::from_reader(file).expect("Could not parse openapi json file");
    let mut generator = Generator::new(
        GenerationSettings::new()
            .with_interface(InterfaceStyle::Builder)
            // Progenitor has an issue where
            // an inner type MUST be set to use with_pre_hook_async
            .with_inner_type(quote! { crate::ClientState })
            .with_pre_hook_async(quote! {
                |_, request: &mut reqwest::Request| {
                    // Synchronously modify the request here (e.g., add headers)
                    // to propagate OpenTelemetry context
                    crate::inject_opentelemetry_context_into_request(request);

                    // Return immediately since we aren't using async functionality
                    Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
                }
            }),
    );

    let tokens = generator
        .generate_tokens(&spec)
        .expect("Could not generate tokens");
    let ast = syn::parse2(tokens).unwrap();
    let mut content = prettyplease::unparse(&ast);

    // Fix clippy::uninlined_format_args warnings by replacing format string patterns
    content = content.replace(
        r#"format!("error converting supplied value for author: {}", e)"#,
        r#"format!("error converting supplied value for author: {e}")"#,
    );
    content = content.replace(
        r#"format!("error converting supplied value for id: {}", e)"#,
        r#"format!("error converting supplied value for id: {e}")"#,
    );
    content = content.replace(
        r#"format!("error converting supplied value for title: {}", e)"#,
        r#"format!("error converting supplied value for title: {e}")"#,
    );
    content = content.replace(
        r#"format!("conversion to `BookCreateIn` for body failed: {}", s)"#,
        r#"format!("conversion to `BookCreateIn` for body failed: {s}")"#,
    );

    let content = format!("#![allow(clippy::all)]\n{content}");

    let mut out_file = std::path::Path::new("src").to_path_buf();
    out_file.push("generated.rs");

    std::fs::write(&out_file, content).unwrap();

    Command::new("cargo")
        .arg("fmt")
        .output()
        .expect("Failed to cargo fmt");
}
