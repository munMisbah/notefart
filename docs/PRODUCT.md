# Produk Notefart

Status: draft. Arah yang sudah disepakati tercatat di [README.md](README.md); rincian berikut adalah usulan produk.

Kondisi implementasi: editor hybrid panel + tab/shared history, browser Enter/parent, frame penuh, ikon bold/solid dan helper berkelompok tersedia. Gap vertikal luar dipangkas, parser/cache Markdown dan profil dev dipercepat tanpa menonaktifkan highlight. YAML/YML/JS/HTML/CSS serta grammar TypeScript/TSX asli ter-highlight. Preferensi pengguna persisten dan clipboard internal/Wayland/X11 tersedia; field editable, F1, Find/Replace dan guard file tetap berlaku. Grammar bukan type checker/LSP; produk tetap editor teks, bukan IDE. Validasi penggunaan harian dan dukungan platform berlanjut. Perintah/batas di [README.md](README.md).

## Tujuan

Membuat editor terminal yang langsung dapat dipakai untuk membuka, mengedit, dan menyimpan teks tanpa mempelajari mode editing khusus. Produk akhir tetap editor teks familiar ala Notepad, **bukan IDE**. Inspirasi superfile digunakan untuk hierarki visual dan layout, bukan untuk menyalin produk tersebut.

Pengguna utama: orang yang nyaman bekerja di terminal tetapi menginginkan interaksi editor desktop, termasuk mouse.

## Alur utama

1. Jalankan Notefart dengan folder, file, atau tanpa argumen.
2. Buka file melalui explorer atau dialog path yang juga bisa dipakai dengan keyboard.
3. Edit teks, pilih bagian teks, copy/paste, dan undo/redo.
4. Buka file lain sebagai tab di panel tujuan.
5. Split editor untuk membandingkan file atau melihat bagian berbeda dari dokumen yang sama.
6. Simpan perubahan, lalu tutup tab, panel, atau aplikasi tanpa kehilangan perubahan secara diam-diam.

## Usulan cakupan awal balanced

| Area | Kebutuhan |
| --- | --- |
| Editing | Insert/delete, navigasi, seleksi, undo/redo, indentasi, pencarian dan penggantian dalam dokumen |
| File | Open, save, Save As, dokumen baru, penanda perubahan, penanganan kegagalan I/O |
| Navigasi | Browser explorer dengan parent `..`/Backspace, Enter masuk folder, Left/Right tree expandable, tab per panel dan split/resize panel |
| Input | Keyboard lengkap; klik, seleksi drag, double-click kata, dan wheel |
| Tampilan | Frame tiap panel, ikon kategori bold/solid, background dasar seragam, syntax highlighting, nomor baris, fokus jelas, helper kelompok `[tombol] Aksi` dan bantuan F1; tema/profil ikon mengikuti [DESIGN.md](DESIGN.md) |
| Clipboard | Internal atau sistem lokal Wayland/X11 jika helper tersedia; status backend/kegagalan jujur, Auto SSH internal, tanpa OSC 52 |
| Keselamatan | Konfirmasi perubahan yang belum disimpan dan deteksi konflik file eksternal saat save |
| Preferensi | Tema, warna, ikon dan mode clipboard melalui konfigurasi pengguna; override CLI dan penyimpanan eksplisit, bukan konfigurasi proyek atau snapshot sesi |

Batas jumlah panel ditentukan dari hasil prototipe dan ukuran terminal, bukan angka arbitrer. Split ditolak dengan penjelasan jika ruang minimum tidak cukup.

## Usulan batas dukungan awal

- Linux menjadi lingkungan validasi pertama; dukungan OS lain belum dijanjikan.
- Editing awal berfokus pada teks UTF-8, dengan atau tanpa BOM, serta line ending LF/CRLF yang dipertahankan saat save.
- Input biner atau encoding yang belum didukung tidak boleh dibuka sebagai teks yang diam-diam merusak byte saat disimpan.
- File dengan mixed newline/lone CR dibuka read-only pada batch editor, dengan penjelasan dan tanpa normalisasi otomatis.
- Batas operasional: load/save 64 MiB, bracketed paste 1 MiB, field path/Find/Replace 4096 byte UTF-8 dan listing 10.000 entri per direktori. Batas field memperhitungkan normalisasi dan seleksi yang diganti. Highlight dibudget 200 baris/update dengan batas 16 KiB/baris; pelanggaran memunculkan peringatan dan plaintext, bukan mengubah isi. Ini bukan jaminan latensi/RSS.
- Pengukuran lokal tersedia di [ROADMAP.md](ROADMAP.md#hasil-geometri-sparse-untuk-baris-panjang). Cache geometri mempercepat navigasi/render/edit berulang pada baris panjang, tetapi lookup dingin masih perlu membaca prefix; batas 64 MiB tetap operasional, bukan SLA. Teks yang terlihat setelah jump tidak berarti highlight seluruh prefix selesai.
- Integrasi IDE penuh bukan target akhir. Fitur tambahan dinilai dari manfaatnya untuk editing teks, bukan untuk mengejar paritas IDE.
- Tidak ada recovery setelah crash sebelum fitur recovery benar-benar diimplementasikan dan diuji.
- Session restore dicatat sebagai fitur tambahan dan belum tersedia; cakupan rencana serta batasnya ada di [ROADMAP.md](ROADMAP.md#fitur-tambahan--session-restore).

## Kriteria penerimaan produk

- Alur utama dapat diselesaikan dengan keyboard saja maupun kombinasi keyboard dan mouse.
- Tab dan panel tidak membuat salinan dokumen yang isinya saling menyimpang.
- Undo/redo bekerja pada dokumen, termasuk ketika dokumen ditampilkan di beberapa panel.
- Kegagalan save mempertahankan isi buffer dan status belum tersimpan.
- File yang berubah di luar aplikasi tidak ditimpa tanpa keputusan pengguna.
- Menutup tampilan terakhir dokumen yang berubah menawarkan Save, Discard, atau Cancel.
- Tidak ada auto-save, eksekusi kode proyek, atau pengiriman isi dokumen tanpa fitur dan tindakan yang eksplisit.
- Tampilan tetap dapat digunakan setelah resize, termasuk ketika terminal terlalu kecil untuk layout normal.

## Keputusan yang masih terbuka

- Persetujuan palet Dark/Light, profil ikon dan rekomendasi font di DESIGN.md; minimum ukuran panel dan batas dukungan terminal. Font diatur melalui terminal, bukan dipasang atau diganti oleh aplikasi.
- Matriks clipboard lokal/SSH dan default izin OSC 52.
- Shortcut final beserta bentrok dengan terminal atau multiplexer.
- Batas ukuran file dan strategi dokumen besar.
- Detail pengelompokan undo dan perilaku seleksi setelah edit dari panel lain.

Urutan pembuktiannya ada di [ROADMAP.md](ROADMAP.md).
