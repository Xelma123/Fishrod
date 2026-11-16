---
name: devral
description: "Yeni bir oturumda projeyi devralır. DEVIR.md'yi ve belgeleri okur, git durumuyla karşılaştırır, anladığı durumu kullanıcıya anlatır ve sıradaki adımı önerir."
disable-model-invocation: true
---

# Projeyi devral

`DEVIR.md` bu oturuma `CLAUDE.md` üzerinden zaten yüklendi. Amaç: önceki oturumun bıraktığı yerden, projeye tam hakim bir rehber ve proje ortağı olarak devam etmek.

Kullanıcının ek notu (varsa): $ARGUMENTS

## 1. Oku

- `DEVIR.md`'yi dikkatle oku.
- Ardından `docs/01-vizyon.md`, `docs/03-yol-haritasi.md` ve `docs/06-kararlar.md`.
- "Sıradaki adım önerisi" hangi belgeleri ya da kaynak dosyaları gerektiriyorsa onları da oku.

## 2. Notu gerçekle karşılaştır

- Git komutu çalıştırma (karar #18). Kullanıcı bir git çıktısı paylaştıysa DEVIR.md'deki son commit ile karşılaştır.
- Kod varsa `cargo test --release` çalıştır. `bench` varsa düğüm sayısını DEVIR.md'deki ile karşılaştır.

## 3. Kullanıcıya anlat (Türkçe, kısa)

- Anladığın durum, en fazla 8 madde: neredeyiz, son ne yapıldı, yarım kalan ne.
- DEVIR.md ile gerçek durum arasındaki farklar (varsa).
- Sıradaki adım önerin ve gerekçesi; varsa bir alternatif.
- Netleştirmek istediğin sorular.

## 4. Bekle

Kullanıcının yönlendirmesini bekle; onay almadan kod yazma.
