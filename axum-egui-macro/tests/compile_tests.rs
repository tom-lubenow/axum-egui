//! Compile-fail tests for the #[server] macro.
//!
//! These tests verify that invalid uses of the macro produce helpful error messages.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
