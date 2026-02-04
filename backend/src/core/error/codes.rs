// Error codes for API responses
// Each code is unique and helps with debugging and client-side error handling

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ErrorCode {
    // Authentication errors (AUTH_xxx)
    AuthMissingHeader,
    AuthInvalidHeader,
    AuthInvalidToken,
    AuthExpiredToken,
    AuthInvalidCredentials,
    AuthUserNotFound,
    AuthPasswordTooWeak,
    AuthRateLimited,
    
    // Server errors (SRV_xxx)
    ServerNotFound,
    ServerAlreadyRunning,
    ServerNotRunning,
    ServerStartFailed,
    ServerStopFailed,
    ServerDirMissing,
    ServerInstalling,
    
    // File system errors (FS_xxx)
    FileNotFound,
    FileReadError,
    FileWriteError,
    FileDeleteError,
    FileMoveError,
    FilePathInvalid,
    FileAccessDenied,
    
    // Database errors (DB_xxx)
    DatabaseConnection,
    DatabaseQuery,
    DatabaseMigration,
    
    // Backup errors (BKP_xxx)
    BackupNotFound,
    BackupCreateFailed,
    BackupRestoreFailed,
    BackupDeleteFailed,
    
    // Validation errors (VAL_xxx)
    ValidationFailed,
    InvalidInput,
    MissingRequiredField,
    
    // System errors (SYS_xxx)
    InternalError,
    ServiceUnavailable,
    ConfigurationError,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Auth
            ErrorCode::AuthMissingHeader => "AUTH_001",
            ErrorCode::AuthInvalidHeader => "AUTH_002",
            ErrorCode::AuthInvalidToken => "AUTH_003",
            ErrorCode::AuthExpiredToken => "AUTH_004",
            ErrorCode::AuthInvalidCredentials => "AUTH_005",
            ErrorCode::AuthUserNotFound => "AUTH_006",
            ErrorCode::AuthPasswordTooWeak => "AUTH_007",
            ErrorCode::AuthRateLimited => "AUTH_008",
            
            // Server
            ErrorCode::ServerNotFound => "SRV_001",
            ErrorCode::ServerAlreadyRunning => "SRV_002",
            ErrorCode::ServerNotRunning => "SRV_003",
            ErrorCode::ServerStartFailed => "SRV_004",
            ErrorCode::ServerStopFailed => "SRV_005",
            ErrorCode::ServerDirMissing => "SRV_006",
            ErrorCode::ServerInstalling => "SRV_007",
            
            // File system
            ErrorCode::FileNotFound => "FS_001",
            ErrorCode::FileReadError => "FS_002",
            ErrorCode::FileWriteError => "FS_003",
            ErrorCode::FileDeleteError => "FS_004",
            ErrorCode::FileMoveError => "FS_005",
            ErrorCode::FilePathInvalid => "FS_006",
            ErrorCode::FileAccessDenied => "FS_007",
            
            // Database
            ErrorCode::DatabaseConnection => "DB_001",
            ErrorCode::DatabaseQuery => "DB_002",
            ErrorCode::DatabaseMigration => "DB_003",
            
            // Backup
            ErrorCode::BackupNotFound => "BKP_001",
            ErrorCode::BackupCreateFailed => "BKP_002",
            ErrorCode::BackupRestoreFailed => "BKP_003",
            ErrorCode::BackupDeleteFailed => "BKP_004",
            
            // Validation
            ErrorCode::ValidationFailed => "VAL_001",
            ErrorCode::InvalidInput => "VAL_002",
            ErrorCode::MissingRequiredField => "VAL_003",
            
            // System
            ErrorCode::InternalError => "SYS_001",
            ErrorCode::ServiceUnavailable => "SYS_002",
            ErrorCode::ConfigurationError => "SYS_003",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl serde::Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
