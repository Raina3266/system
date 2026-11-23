mod cli {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    use clap::{CommandFactory, Parser, error::ErrorKind};

    use crate::{Action, Cli};

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("ocr-screenshot").chain(arguments.iter().copied()))
    }

    #[test]
    fn no_arguments_select_the_ocr_workflow() {
        assert_eq!(parse(&[]).unwrap().action, None);
    }

    #[test]
    fn help_flags_display_generated_help() {
        for argument in ["-h", "--help"] {
            let error = parse(&[argument]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            assert_eq!(error.exit_code(), 0);
            let help = error.to_string();
            assert!(help.contains("Usage: ocr-screenshot"));
            assert!(help.contains("--help"));
            assert!(help.contains("copy it to the clipboard"));
        }
    }

    #[test]
    fn legacy_bare_help_is_an_explicit_subcommand() {
        assert_eq!(parse(&["help"]).unwrap().action, Some(Action::Help));
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("Usage: ocr-screenshot"));
        assert!(help.contains("copy it to the clipboard"));
        assert_eq!(help, parse(&["-h"]).unwrap_err().to_string());
        assert!(help.contains("--help"));
    }

    #[test]
    fn rejects_unknown_flags_and_positional_arguments() {
        for arguments in [
            &["--unknown"][..],
            &["unexpected"],
            &["unexpected", "help"],
            &["help", "extra"],
            &["help", "--unknown"],
        ] {
            assert_eq!(parse(arguments).unwrap_err().exit_code(), 2);
        }
    }

    #[test]
    fn non_unicode_arguments_are_errors_not_panics() {
        let error = Cli::try_parse_from([
            OsString::from("ocr-screenshot"),
            OsString::from_vec(vec![0xff]),
        ])
        .unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }
}

mod workspace {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use crate::model::Workspace;

    #[test]
    fn workspace_is_private_keeps_filenames_and_removes_all_contents_on_drop() {
        let workspace = Workspace::create().unwrap();
        let image = workspace.image_path();
        let directory = image.parent().unwrap().to_owned();
        assert!(
            directory
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&format!("ocr-screenshot-{}-", std::process::id()))
        );
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(image, directory.join("capture.png"));
        assert_eq!(workspace.output_prefix(), directory.join("output"));
        assert_eq!(workspace.output_path(), directory.join("output.txt"));
        fs::write(&image, b"image").unwrap();
        fs::write(workspace.output_path(), b"recognized text").unwrap();
        fs::create_dir(directory.join("nested")).unwrap();
        fs::write(directory.join("nested/extra"), b"extra").unwrap();

        drop(workspace);
        assert!(!directory.exists());
    }

    #[test]
    fn simultaneous_workspaces_are_unique() {
        let first = Workspace::create().unwrap();
        let second = Workspace::create().unwrap();
        assert_ne!(first.image_path(), second.image_path());
    }

    #[test]
    fn workspace_is_cleaned_up_when_a_workflow_returns_an_error() {
        fn failing_workflow(directory: &mut PathBuf) -> Result<(), &'static str> {
            let workspace = Workspace::create().unwrap();
            *directory = workspace.image_path().parent().unwrap().to_owned();
            fs::write(workspace.image_path(), b"partial capture").unwrap();
            Err("simulated OCR failure")
        }
        let mut directory = PathBuf::new();
        let result = failing_workflow(&mut directory);

        assert_eq!(result, Err("simulated OCR failure"));
        assert!(!directory.exists());
    }
}

mod capture {
    use crate::capture::*;

    #[test]
    fn recognizes_gnome_desktop_variants() {
        assert!(desktop_is_gnome("GNOME"));
        assert!(desktop_is_gnome("ubuntu:GNOME"));
        assert!(desktop_is_gnome("GNOME-Classic"));
        assert!(!desktop_is_gnome("niri"));
    }
}

mod ocr {
    use crate::ocr::*;

    #[test]
    fn clipboard_matches_the_shell_scripts_newline_behavior() {
        assert_eq!(clipboard_payload("hello\n\n"), "hello\n");
        assert_eq!(clipboard_payload(""), "\n");
    }
}
