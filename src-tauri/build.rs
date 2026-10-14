/// Ortak komutlar (mh-app, alt modülden) ve yalnız Android kabuğunun
/// komutları; satır başına bir ad, `#` ile başlayan satırlar yorum.
const SHARED: &str = include_str!("../mailhere/crates/mh-app/commands.txt");
const MOBILE: &str = include_str!("commands.txt");

fn names(list: &str) -> impl Iterator<Item = &str> {
    list.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

fn main() {
    println!("cargo:rerun-if-changed=../mailhere/crates/mh-app/commands.txt");
    println!("cargo:rerun-if-changed=commands.txt");
    // Uygulama komutları için izin tanımları: tek pencere yalnız
    // `capabilities/main.json`da izin verilen komutları çağırabilir. Yeni
    // ortak komut mailhere'de `commands.txt` + `shared_commands!`e, Android
    // komutu buradaki `commands.txt`e ve yetenek dosyasına eklenir.
    // `commands` dilim ömrünü `'static` ister; liste derleme boyunca yaşar.
    let commands: &'static [&'static str] = Box::leak(
        names(SHARED)
            .chain(names(MOBILE))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("Tauri derleme adımı başarısız");
}
