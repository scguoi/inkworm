mod common;

mod sanity {
    use inkworm::judge::{equals, normalize};

    #[test]
    fn lib_exports_work_from_integration() {
        assert!(equals("hello.", "hello"));
        assert_eq!(normalize("  A  B  "), "a b");
    }
}
