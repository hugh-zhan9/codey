pub(crate) fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    crate::clipboard_copy::copy_to_clipboard(text).map(|_| ())
}
