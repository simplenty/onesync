pub use super::store::{Account as Profile, AccountStatus as ProfileStatus, auth_response_path, auth_url_path, is_authenticated};

pub type Account = Profile;
pub type AccountStatus = ProfileStatus;
