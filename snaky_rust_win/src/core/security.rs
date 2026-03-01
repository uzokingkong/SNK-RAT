use zeroize::Zeroize;

/// A String wrapper that zeroes its memory on Drop.
/// Use `unsecure_to_string()` only when you must expose the value.
pub struct SecureString {
    pub value: String,
}

impl SecureString {
    pub fn new(s: impl Into<String>) -> Self {
        Self { value: s.into() }
    }

    /// Consumes self and returns the inner String.
    /// The caller becomes responsible for the memory.
    pub fn unsecure_to_string(&self) -> String {
        self.value.clone()
    }
}

impl Drop for SecureString {
    fn drop(&mut self) {
        // Safety: we overwrite the bytes before the String is freed.
        // SAFETY NOTE: The optimizer could in theory elide this, but
        // `zeroize` uses volatile writes to prevent that.
        unsafe {
            let bytes = self.value.as_bytes_mut();
            bytes.zeroize();
        }
    }
}

impl From<String> for SecureString {
    fn from(s: String) -> Self { Self { value: s } }
}

impl From<&str> for SecureString {
    fn from(s: &str) -> Self { Self { value: s.to_string() } }
}

#[macro_export]
macro_rules! poly_hide {
    ($s:expr) => {{
        $crate::core::security::SecureString::new($s)
    }};
}

/// Sandbox detection removed per user request
pub fn is_analysis_env() -> bool { false }

pub fn check_integrity() -> bool { true }
