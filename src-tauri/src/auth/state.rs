use std::sync::Mutex;
use once_cell::sync::OnceCell;
use zeroize::Zeroize;

/// Global auth state — whether the app is currently unlocked and the master key in memory.
pub struct AuthState {
    pub unlocked: bool,
    pub master_key: Option<[u8; 32]>,  // held in memory while app is unlocked
}

impl AuthState {
    pub fn locked() -> Self {
        Self { unlocked: false, master_key: None }
    }

    pub fn unlock(&mut self, key: [u8; 32]) {
        self.master_key = Some(key);
        self.unlocked = true;
    }

    pub fn lock(&mut self) {
        if let Some(mut key) = self.master_key.take() {
            key.zeroize();  // wipe key from memory on lock
        }
        self.unlocked = false;
    }
}

static AUTH_STATE: OnceCell<Mutex<AuthState>> = OnceCell::new();

pub fn init() {
    AUTH_STATE.set(Mutex::new(AuthState::locked())).ok();
}

pub fn get() -> &'static Mutex<AuthState> {
    AUTH_STATE.get().expect("Auth state not initialized")
}
