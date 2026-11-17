//! `mailto:` adresinden yazma ekranının ön bilgisi (RFC 6068).
//!
//! Başka bir uygulamadaki e-posta bağlantısı (`ACTION_VIEW` / `ACTION_SENDTO`,
//! `mailto:` şeması) Mailhere'nin yazma ekranını alıcılarla açar. Niyet dış
//! uygulamadan gelir, yani **güvenilmez girdidir**: burada yalnız alıcı
//! adresleri alınır, doğrulanır ve sayıca sınırlanır; bozuk ya da düşmanca
//! bir adres hiçbir yolda paniğe yol açmaz, yalnız alıcısız (ya da daha az
//! alıcılı) bir yazma ekranı açılır.
//!
//! Alınanlar: yol kısmındaki adresler (`mailto:a@x.com,b@y.com`), `to`
//! başlık alanı (`?to=c@z.com`) ve niyetin `EXTRA_EMAIL` eki. Yazma rotası
//! (`mh_app::routes::compose_route`) şimdilik yalnız alıcı ve hesap
//! taşıdığından `subject`, `body`, `cc` ve `bcc` alanları **okunmaz**.
//! Adresler günlüğe yazılmaz.

use mh_core::{Address, ComposeMode, ComposePrefill};

/// Bir niyetten alınan en çok alıcı (yazma rotasının sınırıyla aynı).
pub const EN_COK_ALICI: usize = mh_app::routes::MAX_PREFILL_TO;
/// Tek adresin en çok uzunluğu (RFC 5321: 254 bayt).
const ADRES_EN_UZUN: usize = 254;
/// Görünen adın en çok uzunluğu (karakter).
const AD_EN_UZUN: usize = 100;

/// Yüzde kodlamasını çözer (`%40` → `@`). `+` olduğu gibi kalır: `mailto:`
/// adresinde boşluk değil, adresin parçasıdır (`ad+etiket@…`). Bozuk kaçış
/// (`%`, `%4`, `%zz`) olduğu gibi bırakılır; sonuç UTF-8 değilse `None`.
fn coz(s: &str) -> Option<String> {
    fn onaltilik(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let cift = (
                b.get(i + 1).copied().and_then(onaltilik),
                b.get(i + 2).copied().and_then(onaltilik),
            );
            if let (Some(ust), Some(alt)) = cift {
                out.push((ust << 4) | alt);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Adres listesini parçalar: virgül ve noktalı virgül ayırır; tırnak ve açılı
/// ayraç içindekiler ayırmaz (`"Demir, Ayşe" <ayse@ornek.com>`).
fn parcalar(liste: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut tirnak, mut ayrac, mut bas) = (false, false, 0);
    for (i, c) in liste.char_indices() {
        match c {
            '"' => tirnak = !tirnak,
            '<' if !tirnak => ayrac = true,
            '>' if !tirnak => ayrac = false,
            ',' | ';' if !tirnak && !ayrac => {
                out.push(&liste[bas..i]);
                bas = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&liste[bas..]);
    out
}

/// Adres yazılmaya değer mi: tek `@`, boş olmayan yerel kısım ve alan adı,
/// boşluk / denetim karakteri / ayraç yok, alan adı yalnız harf, rakam, `-`
/// ve `.` (boş etiket yok). Teslim edilebilirliği değil, biçimi denetler.
fn gecerli(eposta: &str) -> bool {
    if eposta.is_empty() || eposta.len() > ADRES_EN_UZUN {
        return false;
    }
    let Some((yerel, alan)) = eposta.rsplit_once('@') else {
        return false;
    };
    if yerel.is_empty() || yerel.contains('@') {
        return false;
    }
    let yasak = |c: char| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '<' | '>' | ',' | ';' | ':' | '"' | '(' | ')' | '[' | ']' | '\\'
            )
    };
    if yerel.chars().any(yasak) {
        return false;
    }
    !alan.is_empty()
        && alan.split('.').all(|etiket| {
            !etiket.is_empty()
                && !etiket.starts_with('-')
                && !etiket.ends_with('-')
                && etiket.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
}

/// Tek alıcı: `adres` ya da `Ad <adres>`; geçersizse `None`.
fn alici(parca: &str) -> Option<Address> {
    let p = parca.trim();
    let (ad, eposta) = match (p.rfind('<'), p.rfind('>')) {
        (Some(a), Some(k)) if a < k => (&p[..a], p[a + 1..k].trim()),
        _ => ("", p),
    };
    if !gecerli(eposta) {
        return None;
    }
    let ad = mh_mime::clean_display_name(ad.trim().trim_matches('"'));
    let ad: String = ad.trim().chars().take(AD_EN_UZUN).collect();
    Some(Address::new(
        Some(ad.trim()).filter(|a| !a.is_empty() && *a != eposta),
        eposta,
    ))
}

/// Çözülmüş listedeki geçerli alıcıları ekler; aynı adres (büyük-küçük harf
/// ayrımsız) bir kez, toplam en çok [`EN_COK_ALICI`].
fn ekle(out: &mut Vec<Address>, liste: &str) {
    for parca in parcalar(liste) {
        if out.len() >= EN_COK_ALICI {
            return;
        }
        let Some(a) = alici(parca) else {
            continue;
        };
        let var = out
            .iter()
            .any(|o| o.email.to_lowercase() == a.email.to_lowercase());
        if !var {
            out.push(a);
        }
    }
}

/// `mailto:` adresindeki alıcılar: yol kısmı ve `to` başlık alanları, yazıldığı
/// sırayla. `mailto:` ile başlamayan ya da bozuk adres boş liste verir.
pub fn alicilar(adres: &str) -> Vec<Address> {
    let mut out = Vec::new();
    let adres = adres.trim();
    let sema = "mailto:";
    let Some(bas) = adres.get(..sema.len()) else {
        return out;
    };
    if !bas.eq_ignore_ascii_case(sema) {
        return out;
    }
    let kalan = &adres[sema.len()..];
    // Parça (`#…`) adresin parçası değildir.
    let kalan = kalan.split('#').next().unwrap_or_default();
    let (yol, sorgu) = kalan.split_once('?').unwrap_or((kalan, ""));
    if let Some(yol) = coz(yol) {
        ekle(&mut out, &yol);
    }
    for alan in sorgu.split('&') {
        if out.len() >= EN_COK_ALICI {
            break;
        }
        let (ad, deger) = alan.split_once('=').unwrap_or((alan, ""));
        if !coz(ad).is_some_and(|a| a.trim().eq_ignore_ascii_case("to")) {
            continue;
        }
        if let Some(deger) = coz(deger) {
            ekle(&mut out, &deger);
        }
    }
    out
}

/// Niyetten yazma ekranının ön bilgisi: yeni posta, `mailto:` adresindeki ve
/// `EXTRA_EMAIL` ekindeki alıcılarla. Adres boşsa ("Yeni posta" kısayolu) ya
/// da hiçbir alıcı geçerli değilse boş bir yazma ekranı.
pub fn on_bilgi(adres: &str, ek: &[String]) -> ComposePrefill {
    let mut to = alicilar(adres);
    for e in ek {
        ekle(&mut to, e);
    }
    ComposePrefill {
        mode: ComposeMode::New,
        message_id: None,
        to,
        account_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adresler(adres: &str) -> Vec<String> {
        alicilar(adres).into_iter().map(|a| a.email).collect()
    }

    #[test]
    fn tek_ve_cok_alici_yol_ve_to_alani() {
        assert_eq!(adresler("mailto:ayse@ornek.com"), vec!["ayse@ornek.com"]);
        assert_eq!(
            adresler("mailto:ayse@ornek.com,ali@ornek.org"),
            vec!["ayse@ornek.com", "ali@ornek.org"]
        );
        assert_eq!(
            adresler("mailto:ayse@ornek.com?to=ali@ornek.org&subject=Selam&to=can%40ornek.net"),
            vec!["ayse@ornek.com", "ali@ornek.org", "can@ornek.net"]
        );
        // Yol boş, alıcı yalnız `to` alanında; alan adı büyük harfle de olur.
        assert_eq!(
            adresler("mailto:?TO=ayse@ornek.com"),
            vec!["ayse@ornek.com"]
        );
        // Şema büyük harfle, başta ve sonda boşluk.
        assert_eq!(
            adresler("  MAILTO:ayse@ornek.com \n"),
            vec!["ayse@ornek.com"]
        );
    }

    #[test]
    fn yuzde_kodlamasi_cozulur_arti_isareti_korunur() {
        assert_eq!(
            adresler("mailto:ay%C5%9Fe%40%C3%B6rnek.com"),
            vec!["ayşe@örnek.com"]
        );
        // `+` boşluk değildir (alt adres); kodlanmış virgül listeyi böler.
        assert_eq!(
            adresler("mailto:ayse+liste@ornek.com"),
            vec!["ayse+liste@ornek.com"]
        );
        assert_eq!(
            adresler("mailto:?to=a%40ornek.com%2Cb%40ornek.com"),
            vec!["a@ornek.com", "b@ornek.com"]
        );
        assert_eq!(coz("%"), Some("%".into()));
        assert_eq!(coz("a%4"), Some("a%4".into()));
        assert_eq!(coz("%zz%40"), Some("%zz@".into()));
        assert_eq!(coz("%ff"), None, "UTF-8 olmayan bayt");
    }

    #[test]
    fn gorunen_ad_alinir_ve_temizlenir() {
        let to = alicilar("mailto:%22Demir%2C%20Ay%C5%9Fe%22%20%3Cayse%40ornek.com%3E");
        assert_eq!(
            to,
            vec![Address::new(Some("Demir, Ayşe"), "ayse@ornek.com")]
        );
        // Satır sonu taşıyan ad başlık eklemeye kapı açmaz.
        let to = alicilar("mailto:Evil%0D%0ABcc%3A%20x%40y.z%20%3Ce%40ornek.com%3E");
        assert_eq!(to.len(), 1);
        assert_eq!(to[0].email, "e@ornek.com");
        let ad = to[0].name.as_deref().unwrap_or_default();
        assert!(!ad.contains('\r') && !ad.contains('\n'), "{ad:?}");
        // Adı adresle aynı olan alıcıda ad taşınmaz.
        let to = alicilar("mailto:ayse@ornek.com%20%3Cayse@ornek.com%3E");
        assert_eq!(to, vec![Address::new(None, "ayse@ornek.com")]);
    }

    #[test]
    fn bozuk_adres_bos_liste_verir_panik_yok() {
        for bozuk in [
            "",
            "mailto:",
            "mailto:?",
            "mailto:???&&&===",
            "mailto:%",
            "mailto:%%%%",
            "mailto:%ff%fe",
            "mailto:@",
            "mailto:@ornek.com",
            "mailto:ayse@",
            "mailto:ayse",
            "mailto:ayse@@ornek.com",
            "mailto:ayse@ornek..com",
            "mailto:ayse@.com",
            "mailto:ayse@-ornek.com",
            "mailto:ayse%20demir@ornek.com",
            "mailto:ayse@ornek.com%0D%0ABcc:x@y.z",
            "mailto:<>",
            "mailto:<<<>>>",
            "mailto:\"\"\"",
            "mailto:,,,;;;",
            "mailto:javascript:alert(1)",
            "http://ornek.com/?to=ayse@ornek.com",
            "tel:+905551112233",
            "mail",
            "mailtö:ayse@ornek.com",
            "ğğğğğğğ",
            "\u{0}\u{1}\u{2}",
        ] {
            assert!(alicilar(bozuk).is_empty(), "{bozuk:?}");
        }
        // Çok uzun adres ve çok uzun girdi.
        let uzun = format!("mailto:{}@ornek.com", "a".repeat(300));
        assert!(alicilar(&uzun).is_empty());
        let dev = format!("mailto:?body={}", "%41".repeat(200_000));
        assert!(alicilar(&dev).is_empty());
    }

    #[test]
    fn gecerli_olanlar_bozuklarin_arasindan_secilir() {
        assert_eq!(
            adresler("mailto:bozuk,ayse@ornek.com,,@x,ali@ornek.org;can@ornek.net"),
            vec!["ayse@ornek.com", "ali@ornek.org", "can@ornek.net"]
        );
        // Konu, gövde, cc ve bcc okunmaz.
        assert_eq!(
            adresler("mailto:ayse@ornek.com?cc=ali@ornek.org&bcc=can@ornek.net&subject=to=x@y.z&body=to%3Dx%40y.z"),
            vec!["ayse@ornek.com"]
        );
        // Parça adrese girmez.
        assert_eq!(
            adresler("mailto:ayse@ornek.com#to=x@y.z"),
            vec!["ayse@ornek.com"]
        );
    }

    #[test]
    fn yinelenen_atilir_sayi_sinirlanir() {
        assert_eq!(
            adresler("mailto:Ayse@Ornek.com,ayse@ornek.com?to=AYSE@ORNEK.COM"),
            vec!["Ayse@Ornek.com"]
        );
        let cok = (0..50)
            .map(|i| format!("kisi{i}@ornek.com"))
            .collect::<Vec<_>>()
            .join(",");
        let to = alicilar(&format!("mailto:{cok}?to={cok}"));
        assert_eq!(to.len(), EN_COK_ALICI);
        assert_eq!(to[0].email, "kisi0@ornek.com");
    }

    #[test]
    fn on_bilgi_yeni_posta_ek_alicilarla() {
        let p = on_bilgi(
            "mailto:ayse@ornek.com",
            &[
                "Ali Kaya <ali@ornek.org>".into(),
                "ayse@ornek.com".into(),
                "bozuk".into(),
            ],
        );
        assert_eq!(p.mode, ComposeMode::New);
        assert_eq!(p.message_id, None);
        assert_eq!(p.account_id, None);
        assert_eq!(
            p.to,
            vec![
                Address::new(None, "ayse@ornek.com"),
                Address::new(Some("Ali Kaya"), "ali@ornek.org"),
            ]
        );
        // "Yeni posta" kısayolu: adres yok, boş yazma ekranı.
        let bos = on_bilgi("", &[]);
        assert!(bos.to.is_empty());
        assert_eq!(mh_app::routes::compose_route(&bos), "compose/new/0");
        // Rota alıcıyı taşır; `+` kodlanır, arayüz geri çözer.
        let p = on_bilgi("mailto:ayse+liste@ornek.com", &[]);
        assert_eq!(
            mh_app::routes::compose_route(&p),
            "compose/new/0?to=ayse%2Bliste%40ornek.com"
        );
    }
}
