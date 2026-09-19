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
