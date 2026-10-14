//! Geliştirme: Android kabuğunu masaüstünde açar (`cargo run` ya da
//! `pnpm tauri dev`). Android anahtar kasası burada yoktur; parolalar ve
//! veritabanı anahtarı bellekte tutulur (`MAILHERE_SECRETS=memory`, yalnız
//! geliştirme derlemesinde okunur). Sürüm derlemesi masaüstünde açılmaz.

fn main() {
    // Bellek kipi, kullanıcı başka bir şey seçmediyse (tanıtım kipi de bellek kullanır).
    if std::env::var_os("MAILHERE_SECRETS").is_none() && std::env::var_os("MAILHERE_DEMO").is_none()
    {
        std::env::set_var("MAILHERE_SECRETS", "memory");
    }
    mailhere_lib::run();
}
