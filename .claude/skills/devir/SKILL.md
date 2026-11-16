---
name: devir
description: "Oturumu devreder. DEVIR.md'yi baştan yazar, oturum günlüğüne giriş ekler ve yeni oturumun /devral ile devam edebileceği hale getirir. Bağlam dolduğunda, bir faz bittiğinde ya da kullanıcı oturumu kapatırken kullanılır."
disable-model-invocation: true
---

# Devir notu hazırla

Amaç: Bu oturumu hiç görmemiş yeni bir ajan, yalnızca `DEVIR.md` ile projeye tam hakim olup kaldığımız yerden devam edebilmeli. Not ne kadar iyiyse projenin hafızası o kadar sağlam.

Kullanıcının ek notu (varsa): $ARGUMENTS

## 1. Gerçek durumu topla (tahmin etme)

- Git komutu çalıştırma (karar #18). Son commit'i bu oturumda verdiğin commit komutlarından al; emin değilsen kullanıcıdan `git log --oneline -5` çıktısını iste.
- README'deki "Status" satırı, `docs/03-yol-haritasi.md` işaretleri, `docs/test-gunlugu.md`'nin son satırları
- Kod varsa `cargo test --release` sonucu; `bench` komutu varsa düğüm sayısı (`cargo run --release -- bench`)

## 2. DEVIR.md'yi baştan yaz

Eskisini yamamak yerine güncel durumu yansıtan yeni bir not yaz. Aşağıdaki şablonu kullan.

- **Somut ol:** dosya yolları, tip ve fonksiyon adları, komutlar, sayılar (perft, bench, SPRT), commit hash'leri.
- **Nedenleri yaz:** alınan kararların ve denenip bırakılan yolların gerekçesi, yeni ajan aynı hatayı tekrarlamasın.
- **Yarım iş:** tam olarak nerede kaldığını yaz; hangi dosyada ne eksik, hangi test kırmızı, sıradaki satır ne.
- **Kullanıcı:** bu oturumda öğrendiklerini "Kullanıcı hakkında" bölümüne ekle; tercihleri, neyi anladığı, nerede zorlandığı. Önceki nottaki bilgileri koru, eskiyenleri düzelt.
- **Uzunluk:** en fazla 150 satır. Ayrıntı bir belgedeyse oraya bağlantı ver.

## 3. Günlük ve belgeler

- `docs/oturum-gunlugu.md` dosyasının en üstüne 3-6 satırlık bir giriş ekle: tarih, ne yapıldı, sonuç, sıradaki.
- Yol haritası işaretleri ve test günlüğü güncel değilse güncelle.
- Yeni bir karar alındıysa `docs/06-kararlar.md`'ye eklemeden önce kullanıcıya teyit ettir.

## 4. Kullanıcıya göster

- Devir notunun 5 maddelik özeti
- `DEVIR.md` ve `docs/` git dışında (karar #19); onlar için commit komutu verme. Bu oturumda git'teki dosyalar değişip henüz commit edilmediyse yalnızca onların komutlarını ver.
- Devam etmek için: yeni bir oturum aç (`claude`) ya da `/clear` yaz, sonra `/devral`.

## DEVIR.md şablonu

```markdown
# Devir notu

> Projenin "şu an" hafızası. Her oturum sonunda /devir ile baştan yazılır. Geçmiş oturumlar: docs/oturum-gunlugu.md

**Son güncelleme:** <tarih> · **Son commit:** <başlık, biliniyorsa hash> · **Faz:** <faz ve durumu>

## Proje bir paragrafta
## Neredeyiz
## Son oturumda ne yaptık ve neden
## Kullanıcı hakkında
## Önemli kararlar (tam liste: docs/06-kararlar.md)
## Açık konular ve riskler
## Sıradaki adım önerisi
## Yeni ajan için uyarılar
```
