# Roadmap Notefart

Status: draft tanpa tanggal rilis. Urutan di bawah adalah usulan implementasi balanced, bukan pengurangan fitur yang telah disepakati.

Diagnostik, editor aman, workspace dan hybrid panel + tab/shared view sudah berjalan pada cakupan Linux yang dinyatakan. Tahap berikutnya pemolesan UI/UX dan validasi penggunaan harian. Kompilasi/mockup saja bukan bukti tahap selesai; hasil nyata dicatat di bawah.

Nama executable saat ini **`naf`**; produk dan namespace konfigurasi tetap Notefart/`notefart`. Perintah serta path binary `notefart` pada laporan batch sebelum migrasi di bawah adalah catatan pemeriksaan historis, bukan alias atau instruksi build aktif. Instruksi terkini ada di [README.md](README.md#menjalankan-editor); output binary/artifact lama telah dibersihkan pada migrasi.

## Tahap 0 — Validasi keputusan berisiko

Pekerjaan:
- Tentukan terminal Linux untuk validasi pertama serta skenario lokal, multiplexer, dan SSH yang tersedia.
- Spike raw mode, kombinasi tombol, mouse capture, double-click, drag, wheel, bracketed paste, dan restorasi terminal.
- Bandingkan kandidat rope/highlighting dan periksa lisensi/dependency.
- Coba rendering grapheme, combining mark, emoji, karakter lebar, tab, serta pemetaan klik ke teks.
- Validasi lebar ikon/border pada Nerd Font Mono, font biasa dengan Unicode, dan profil ASCII; catat dukungan truecolor/256/16 warna serta override.
- Validasi clipboard sistem dan opsi OSC 52 tanpa menjadikannya prasyarat editing.

Lulus jika: hasil dan keterbatasan tiap lingkungan dicatat, dependency awal dipilih dengan alasan, serta keputusan dukungan encoding, save link, dan shortcut diperbarui di dokumen terkait. Spike bukan kode produksi otomatis.

### Hasil batch pertama

**Target batch diagnostik tercapai; tahap 0 secara keseluruhan belum ditutup.** Implementasi tersedia melalui `cargo run -- --probe`, `cargo run --release --example text_probe`, dan `cargo run --release --example highlight_probe`. Tidak ada fitur file editing/save, explorer, tab, atau split produksi pada batch ini.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Build debug executable dan release examples | Lolos pada Rust/Cargo 1.99.1, Linux x86_64 |
| Unit test executable setelah perbaikan review | 8 lolos; escaping, batas preview, paste-as-data, key kinds, double-click/hit, resize tanpa render, drag→click |
| Contoh teks release | Boundary Unicode/tab/CRLF, seam grapheme lintas chunk, snapshot, edit/undo/redo dan benchmark lolos |
| Contoh highlight release | UTF-8 coverage, multiline, terminator edit/restore, semantic mapping, EOF tanpa newline lolos |
| CLI tanpa TTY | Ditolak sebelum output kontrol terminal; help/version dan argumen unsupported juga diperiksa |
| PTY 120×36 | Keyboard, Esc/Ctrl+Z, klik/up/double-click/drag, wheel empat arah, paste 50 KB dengan Ctrl+Q/Ctrl+C/OSC lolos |
| Resize PTY | 120×36 → 20×6 → 120×36, tetap hidup dan dapat keluar |
| Exit Ctrl+Q/Ctrl+C | Status 0; termios, alternate screen, mouse dan bracketed paste dipulihkan |
| SIGTERM/SIGHUP/SIGINT saat paste belum lengkap | Status nonzero, nama signal dilaporkan; termios dan mode terminal dipulihkan |
| tmux 3.5a, server terisolasi | Paste berisi Ctrl+Q/OSC tetap data, Ctrl+Q eksplisit keluar status 0 |

PTY memakai TERM=xterm-256color; urutan input/mouse disuntikkan sebagai protokol terminal dan output/state diamati. Ini bukan pengujian klik manusia, glyph font fisik, atau clipboard desktop. Server tmux pengujian sudah dihentikan; sesi pengguna tidak disentuh.

Pada batch diagnostik awal, pembaca thread dipakai agar paste parsial tidak menahan cleanup. Batch editor kemudian menggantinya dengan decoder streaming berbatas pada thread utama untuk editor dan probe; jalur thread/parser paste Crossterm sudah dihapus.

**Masih perlu validasi/keputusan sebelum menutup tahap 0 penuh:**
- Matriks terminal fisik/font, Nerd Font, dan degradasi warna/override; probe saat ini hanya Dark.
- SSH dan OS lain; hasil tmux lokal tidak mewakili keduanya.
- Clipboard sistem/OSC 52 dan shortcut pada matriks terminal lebih luas. Batas ingress paste 1 MiB dan shortcut editor dasar sudah diimplementasikan pada batch editor.
- Panic/setup/syscall fault injection dan suspend/resume; hook panic baru direview statis, suspend eksternal belum didukung.
- Matriks filesystem/platform lebih luas. UTF-8/BOM/LF/CRLF, mixed read-only, dan penolakan save link sudah diterapkan serta diuji pada cakupan batch editor berikut.

Keputusan dependency/pengukuran ada di [ARCHITECTURE.md](ARCHITECTURE.md). Batch editor menambahkan revision/checkpoint dan undo; batch workspace menambahkan generation teks/cache highlight. Shared-view remapping tetap tahap berikutnya.


## Tahap 1 — Editing satu dokumen yang aman

Pekerjaan:
- Lifecycle terminal, event loop, buffer teks, dan satu editor view.
- Input, navigasi, seleksi keyboard/mouse, scroll, transaksi undo/redo, dan paste.
- Dokumen baru, open path, save, Save As, dirty checkpoint, dan dialog keluar.
- Round-trip UTF-8/BOM/LF/CRLF sesuai cakupan yang disetujui, penolakan format unsupported, serta rendering aman.
- Konflik eksternal dan penanganan kegagalan save mengikuti SECURITY.md.

Lulus jika: pengguna dapat membuat/membuka, mengedit, undo/redo, menyimpan, dan keluar tanpa kehilangan perubahan pada skenario normal maupun kegagalan yang diuji. Unicode dan klik cursor tidak saling bergeser pada fixture yang didukung.

### Hasil batch editor satu dokumen

**Kriteria tahap 1 tercapai pada cakupan Linux awal yang dinyatakan**, bukan seluruh matriks terminal/filesystem produk. Tanpa argumen membuka editor; `--probe` tetap diagnostik. Empat worker implementasi dan empat reviewer read-only digunakan; integrator memperbaiki temuan dan memverifikasi hasil.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Build dan unit test executable | 70 test lolos: history/checkpoint/branching, grapheme/cell, parser, format/I/O, metadata, modal dan clipping |
| PTY new → paste Unicode → Save As | Bytes tepat; undo dirty, redo ke checkpoint bersih, exit memulihkan terminal |
| Clipboard dan keyboard | Ctrl+C tidak keluar; Ctrl+A/C/End/V, undo/save, scalar UTF-8, Backspace grapheme, Shift+Right replacement lolos |
| Mouse | Sel kedua CJK snap kiri, double-click memilih kata, drag mengganti seleksi; bytes hasil save tepat |
| BOM/CRLF round-trip | BOM, final newline, edit, serta mode 0640 dipertahankan |
| Dirty dan save gagal | Cancel exit, parent tidak ada, current-path Save As confirmation/cancel dan Discard eksplisit lolos |
| Konflik eksternal | Tidak silent overwrite; Cancel, Save As, Reload eksplisit terbukti. Overwrite eksplisit tercakup unit test |
| New/Open dialog | Open path, New, Cancel dokumen dirty, Discard lalu Open bekerja |
| Unsupported/link | Mixed read-only; symlink save ditolak lalu Save As berhasil; FIFO/invalid UTF-8 ditolak tanpa kontrol terminal |
| Metadata nyata | Mode, uid/gid dan user xattr dipertahankan; perubahan permission setelah copy ditolak oleh regression test |
| Paste besar/fragmentasi | Lebih dari 1 MiB dibuang utuh; Ctrl+Q/C di payload tidak dieksekusi; prefix terfragmentasi tetap data |
| Layar kecil | Gutter 99→100 menjaga cursor pada frame lengkap; modal 8×2 menampilkan Resize/Esc dan menolak blind Discard; resize memulihkan buffer dan save |
| Decoder baru | SIGTERM saat paste parsial memulihkan editor/probe; probe tmux 3.5a terisolasi lolos |

Fixture file, FIFO, link, atribut, dan server tmux dibuat terisolasi lalu dibersihkan. Frame PTY lengkap diamati; glyph/font fisik tidak diklaim terverifikasi. Input, dokumen, I/O dan UI merupakan komponen produksi nyata, bukan contoh benchmark yang otomatis dipindah.

Pada akhir batch tahap 1, explorer/tab/split, search/replace, highlight produksi, Light/profil ikon, clipboard eksternal dan recovery belum tersedia. Batas batch itu: file 64 MiB, paste 1 MiB, dialog 20×6; bukan SLA performa.

Risiko tersisa: final check→rename race, filesystem sinkron lambat, beragam ACL/security label, panic/syscall fault injection, SSH/OS/font/protokol terminal lain. Detail ada di SECURITY.md.


## Tahap 2 — Workspace dan kenyamanan dasar

Pekerjaan:
- Explorer expandable dan penanganan permission error.
- Beberapa tab pada satu panel, fokus keyboard/mouse, penanda dirty, dan close tab.
- Pencarian/penggantian dalam dokumen dengan seleksi hasil dan undo yang benar.
- Highlighting awal, status bar, nomor baris, dan shortcut helper kontekstual.
- Implementasikan token tema, Dark/Light/Terminal, dan profil ikon mengikuti DESIGN.md; periksa kontras, truncation, dan hit target pada state nyata.

Lulus jika: alur buka folder → pilih file → edit → cari/ganti → pindah tab → simpan dapat dilakukan tanpa mouse, lalu dicoba kembali dengan mouse. Pencarian mencakup hasil tidak ditemukan, wrap, dan karakter multibyte; penggantian dapat di-undo. Highlight tidak mengubah teks atau memblokir input secara tidak layak pada fixture ukur.

### Hasil batch workspace satu panel

**Kriteria tahap 2 tercapai pada cakupan Linux/PTY yang dinyatakan.** Enam worker implementasi dan dua reviewer read-only digunakan. Integrator memperbaiki crash ellipsis UTF-8 satu kolom, invariant ekspansi symlink ancestor, scroll explorer yang kembali ke selection dan klik stale setelah navigasi sebelum frame baru.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Format, build dan unit test executable | `cargo fmt`, `cargo test --bin notefart`, `cargo build --bin notefart`: 140 test lolos; build akhir tanpa warning |
| Keyboard folder → file → tab | Expand folder, buka Python, Open Rust additive dan dedup alias symlink lolos |
| Replace nyata | F6 mengganti satu hasil dan menyimpan bytes tepat; F7 Unicode/ZWJ mengganti semua, satu undo memulihkan seluruh hasil dan redo memulihkannya lagi |
| Find | Next/previous, wrap kedua arah, no-match dan query multibyte dibuktikan; multiline/boundary/overlap tercakup unit test |
| Guard search kecil | Resize 39×9 menyembunyikan field/menolak F7; kembali normal dan bytes tidak berubah |
| Mouse explorer | Wheel tetap tergulir setelah redraw tanpa memindahkan fokus; queued click memakai frame lama ditolak, fresh click membuka file benar |
| Wheel editor/frame stale | Queued click setelah wheel ditolak; fresh click dengan gutter 3 digit menuju baris/kolom benar; wheel di header tidak menggulir editor |
| Divider/resize | Drag 28→35 kolom, hide pada lebar 25 dan restore mempertahankan lebar/state |
| Mouse tab overflow | Previous/next mengaktifkan tab yang benar dan close menutup target benar |
| Multi-tab keselamatan | Save As ke target tab lain ditolak; close dirty nonaktif menampilkan dokumen benar; Discard satu lalu Cancel mempertahankan dokumen lain; Save As cancel membatalkan quit |
| Highlight nyata | Warna keyword/string Rust berbeda; Python terlihat; scroll >450 baris menjaga konteks, edit delimiter multiline + undo menginvalidasi/memulihkan warna |
| Batas highlighting | Baris >16 KiB memunculkan warning pause, aplikasi tetap dapat keluar bersih |
| Tema/profil | PTY Dark/Light truecolor, 256, 16; Terminal/auto dan Light/none lolos. ASCII struktur murni ASCII pada fixture ASCII; Unicode/Nerd mengirim glyph tanpa crash |
| Kontras | Unit test menguji teks/syntax >=4.5:1 dan border >=3:1 pada Dark/Light RGB dan xterm-256; ANSI pengguna tidak dijamin |
| CLI/cleanup | Help dan invalid/conflicting flags tanpa output kontrol; setiap sesi selesai memulihkan termios, alternate screen, mouse dan bracketed paste |

Ini pengamatan protokol terminal/pyte dan bytes file nyata, bukan klik manusia atau pembuktian font fisik. Glyph Nerd Font, SSH/emulator lain, palette ANSI kustom dan matriks filesystem tetap belum tervalidasi. Tidak mengulang shortcut lama hanya untuk mengonfirmasi laporan pengguna.

Batas yang ditambahkan batch tahap 2: listing 10.000 entri/direktori, field search 4096 byte, jendela Replace All 64 MiB, highlight 200 baris/update/16 KiB per baris dan dialog search 40×10. Bukan SLA latensi/RSS. Pada akhir batch itu split/shared-view belum tersedia; clipboard sistem, recovery dan konfigurasi persisten tetap belum tersedia.


## Tahap 3 — Hybrid panel dan tab

Pekerjaan:
- Split tree dua orientasi dan tab per panel.
- Dokumen shared dengan cursor/selection/viewport independen.
- Pemetaan posisi seluruh view setelah edit dan undo/redo.
- Fokus panel, pembukaan file ke panel tujuan terakhir, resize divider, dan penutupan panel.
- Penanganan terminal kecil tanpa membuang buffer atau layout tersimpan.

Lulus jika: file yang sama dibuka di dua panel, diedit dari keduanya, di-undo, disimpan, lalu salah satu panel ditutup dengan state tetap konsisten. Dokumen berbeda tetap memiliki undo independen. Jalur keyboard dan mouse, termasuk resize terminal, keduanya dibuktikan.

### Hasil batch hybrid panel + tab

**Kriteria tahap 3 tercapai pada cakupan Linux/PTY yang dinyatakan.** Empat worker implementasi dan empat reviewer read-only digunakan. Integrator menyelesaikan borrow scopes/gating API produksi, menghapus test wording/alokasi incidental, dan memperbaiki hit stale setelah scroll keyboard/autoscroll serta sinkronisasi resize sebelum shortcut split.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Format/build/test | `cargo fmt`, `cargo test --bin notefart`, `cargo build --bin notefart`; 178 test lolos, build tanpa warning |
| Shared edit/history | Dua panel menampilkan teks yang sama, edit dan undo/redo dari keduanya tersinkron; cursor independen dan bytes save tepat |
| Dokumen lain dan alias | History dokumen lain tidak berubah oleh undo shared; alias mengaktifkan view lokal tanpa membuat buffer divergen |
| Save As shared | Path sendiri meminta overwrite, bukan collision; rename memperbarui kedua caption/path/checkpoint. Target dokumen lain tetap ditolak dalam unit test |
| Replace All shared | F7 terlihat di kedua panel dan dapat di-undo dari panel lain |
| Nested layout | F8 kiri-kanan dan F9 atas-bawah menjadi tiga panel; Alt+Shift+Arrow dan mouse drag mengubah divider sesuai orientasi |
| Layar kecil | Lebar 20 menyisakan panel aktif, split baru ditolak; kembali 120 memulihkan tree/ratio. F10 menampilkan panel tersembunyi tanpa membuang view |
| Mouse tab | Klik tab memilih pane/view benar; close shared view mempertahankan dokumen; close tab terakhir panel nonaktif menghapus panel saja |
| Wheel/drag | Wheel pada pane nonaktif tidak memindahkan fokus. Drag menyeberang divider tetap memilih teks view asal; bytes save, cursor dan anchor viewport view lain tepat |
| Syntax shared | View awal dan akhir >450 baris berbagi cache; edit terminator komentar mengubah warna, undo dari pane lain memulihkan keyword |
| Dirty shared/final | Close shared dirty view tanpa prompt; last-view close memakai guard. Cancel mempertahankan buffer |
| F12 beberapa dokumen | Save As cancel, Discard satu, save gagal, Cancel lalu save sukses bekerja; shared survivor tidak dibuang |
| Quit shared | Dokumen dirty dengan dua view hanya meminta save sekali; save/exit dan termios/mouse/alternate-screen/bracketed-paste dipulihkan |
| Engine/layout boundary | Unit test meliputi Unicode seam/affinity, hidden views, sparse range union, nested minima/ratio, remove/collapse dan ekstrem u16 |

Fixture/runtime terisolasi; pengamatan memakai protokol terminal/pyte dan bytes file nyata, bukan font fisik. Undo/redo shared tetap satu history per dokumen. Replace All adalah satu splice jendela komposisi: posisi di dalam jendela mengikuti affinity range tersebut, bukan transform per-match.

Visual belum final. Tahap 4 mencakup spacing/proporsi/helper/dialog dan validasi penggunaan harian, tanpa menghapus arah hybrid editor. SSH, emulator/font fisik, ACL/security label, concurrent-writer race, clipboard sistem dan recovery tetap memiliki batas sebelumnya.


## Tahap 4 — Kesiapan penggunaan harian

Pekerjaan:
- Rapikan tampilan berdasarkan penggunaan nyata, bukan menambah dekorasi tanpa fungsi.
- Uji keseluruhan alur produk dan matriks terminal yang dipilih.
- Buktikan matriks visual Dark/Light/Terminal/tanpa warna dan Nerd Font/Unicode/ASCII, termasuk seleksi, tab dirty, dialog error, serta terminal kecil.
- Ukur input/render dan memori pada ukuran file/baris berbeda; tetapkan batas dukungan berdasarkan hasil.
- Lengkapi penanganan clipboard unavailable, permission denied, file berubah/hilang, dan kegagalan save.
- Review keamanan dependency serta skenario minimum SECURITY.md.
- Tulis panduan instalasi, shortcut, dukungan terminal, dan keterbatasan berdasarkan aplikasi yang benar-benar tersedia.

Lulus jika: seluruh kriteria PRODUCT.md terpenuhi pada cakupan dukungan yang dinyatakan, tidak ada kehilangan data yang diketahui pada alur yang didukung, dan dokumentasi tidak menjanjikan kemampuan yang belum ada.

### Hasil batch pemolesan UI/UX

Batch pertama selesai; kriteria kesiapan penggunaan harian di atas **belum seluruhnya ditutup**.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Format/test/build | `cargo fmt --all`, 204 unit test dan build binary lolos |
| Field editable | Caret/seleksi Unicode termasuk ZWJ; replacement pada batas 4096 byte, penolakan oversized/control atomik dan clipboard internal lintas Path/Find terbukti |
| Mouse dan konfirmasi | Drag mengganti komponen nama file lalu membuka target berbeda; klik Open/Help/Return/Accept/Cancel/Save menjaga dirty guard dan menghasilkan bytes save tepat |
| Konteks bantuan | Editor, explorer, Find/Replace, path dan konfirmasi dirty dapat kembali ke konteks semula; New/Quit tidak bocor melalui help |
| Save As tertunda | F1 mempertahankan seleksi field dan continuation Quit/Save As melewati scroll/resize; save sukses menulis target yang benar dan keluar |
| Help kecil/resize | End pada 20×6 lalu diperbesar 100×24 langsung menampilkan page terakhir yang benar; wheel dan return melalui mouse terbukti |
| Search shared | Query/replacement CRLF/CR menjadi LF; navigasi caret dan F1 mempertahankan match pertama; replace/save/undo pada split shared menghasilkan bytes tepat |
| Visual dan escape | Metadata CRLF+BOM, seleksi field dan escape OSC/Bidi terbukti. Mode none tidak mengirim warna SGR eksplisit |

Profil PTY yang dijalankan: Dark/truecolor/ASCII, Light/256/Unicode, Terminal/none/ASCII, Dark/16/Nerd. Ini pemeriksaan protokol/cell Linux `TERM=xterm-256color`, bukan matriks emulator/font fisik. Regresi unit juga meliputi caret setelah penggabungan grapheme, Unicode tiga sel, gap kata bantuan, dan offset help sesudah resize.

Layout/helper/dialog kini lebih ringkas, tetapi pengukuran performa/memori, matriks SSH/emulator/font, audit dependency dan kriteria rilis lain tetap terbuka. Tidak ada tambahan clipboard sistem, recovery atau konfigurasi persisten.

### Hasil redesign visual bergaya superfile

Permintaan perubahan visual menyeluruh diwujudkan melalui kartu berbingkai, ikon dan hierarki fokus, bukan penambahan fitur file manager palsu.

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Format/test/build | `cargo fmt --all`, 217 unit test dan build binary lolos |
| Frame dan ikon | Explorer/editor memakai frame penuh; header/dock normal berbingkai, tab memakai ikon kategori. Alignment sudut dan border diperiksa pada empat profil PTY |
| Border dan gutter | Border editor/explorer hanya memfokuskan; gutter menuju baris benar. Insert/save/undo menghasilkan bytes file tepat |
| Wheel dan drag | Wheel pada border explorer/editor nonaktif tidak mencuri fokus; gap kolom dan sidebar dapat di-drag dengan ukuran frame diperhitungkan |
| Inner viewport | PageDown pada nested split memakai tinggi teks sebenarnya; klik/edit/save/undo setelah scroll menghasilkan bytes tepat |
| Compact dan collapse | Split tambahan pada 100×18 memadatkan chrome global tanpa menyembunyikan panel yang masih muat. Split mustahil ditolak; lebar 20 mempertahankan panel aktif dan F10 menjangkau panel tersembunyi; resize memulihkan tree/ratio |
| Tab dan minimum | Close shared tab mempertahankan view lain/fokus; editor 12×6 dengan gutter tiga digit masih memiliki teks/caret. Ukuran lebih kecil menampilkan notice aman dan dapat dipulihkan |
| Dock dan warna | F1 dari dock kembali ke caret/konteks semula. Gap Dark/Light memakai canvas eksplisit; Terminal/none tidak mengirim warna SGR eksplisit |
| Inspeksi visual | Grid PTY empat editor bertingkat diraster dengan JetBrainsMono Nerd Font Mono sementara; font/config sistem tidak diubah |

Profil PTY: Dark/truecolor/Unicode, Light/256/Unicode, Terminal/none/ASCII dan Dark/16/Nerd. Pemeriksaan cell/protokol serta raster font lokal bukan validasi emulator/font fisik. Seluruh sesi mengembalikan termios, mouse, alternate screen dan bracketed paste. Fixture pengujian dibersihkan.

Modul `chrome` menjadi sumber bersama minimum/geometry renderer/controller. Frame, ikon dan spacing tersedia untuk evaluasi; kriteria kesiapan rilis tahap 4 dan batas dukungan sebelumnya tetap berlaku.

### Hasil helper berkelompok, ikon solid dan browser folder

| Pemeriksaan yang dijalankan | Hasil |
| --- | --- |
| Format/unit/build | Format dan build sukses. Suite 230 tes dijalankan: 198 di luar Explorer lolos; empat fixture Explorer baru dikoreksi dan regresi retry ditambahkan. Rerun `cargo test --bin notefart explorer`: 33/33 lolos (231 jalur unit tercakup keseluruhan) |
| Helper | Status berbingkai 3 baris, helper biasa di bawahnya. Klik Open/Save/Up/Help melakukan aksi nyata; separator tidak mengubah caret/state. Label normal dan keycap tegas, tanpa hit parsial |
| Folder browsing | Enter masuk, Backspace/klik parent/helper Up naik dan memilih folder sebelumnya; klik folder tetap toggle. Folder kosong dapat ditinggalkan, `/` no-op, F5/helper Refresh membaca root terbaru. Startup permission error lalu F5 recovery memperbarui header canonical sebelum naik ke parent fisik |
| Integritas | Dokumen dirty shared bertahan melalui browsing; klik Save, undo dan save menulis bytes tepat ke path semula. CWD proses tetap |
| Directory race | Directory fixture diganti symlink setelah listing; Enter ditolak, header/tree/dokumen lama tetap. Sesudah dipulihkan, refresh berhasil |
| Geometry/style | Wheel di parent row menggulir tree tanpa mencuri fokus; baris parent tetap. Minimum 12×6 dan restore split terbukti. Semua background biasa memakai canvas; hanya seleksi/fokus semantik berbeda |
| Ikon dan profil | Glyph Material Design solid diverifikasi metadata upstream, ikon bold pada cell nyata. Dark/truecolor/Unicode, Light/256/Unicode, Terminal/none/ASCII, Dark/16/Nerd menjaga alignment/helper/modal isolation; none tanpa warna SGR eksplisit |
| Syntax fixture | Tujuh file `testfile` dibuka tanpa mutasi bytes. YAML/YML, JS, HTML, CSS memiliki warna token; TS/TSX plaintext sesuai ketiadaan bundled grammar |
| Visual | Grid CSS/JS/YAML nested diraster dengan font Nerd Mono sementara; font/config sistem tidak diubah |

Seluruh delapan sesi PTY mengembalikan termios, mouse, alternate screen dan bracketed paste. Fixture runtime/probe sementara dibersihkan; `testfile` sengaja dipertahankan sesuai permintaan. Belum ada history Back/Forward, operasi file manager, grammar TypeScript/TSX maupun validasi emulator/font fisik universal. Kriteria tahap 4 lainnya tetap terbuka.

### Hasil performa Markdown, grammar TS/TSX dan layout rapat

Penyebab yang ditangani: regex-fancy menjalankan cold regex work di thread UI, viewport lama langsung dibuang sehingga scroll balik mem-parsing ulang, dan renderer dev tanpa optimasi memakai CPU tinggi. Cutover: Oniguruma native, FIFO recent terbatas, slice kooperatif, satu slice per langkah render/input, serta profil dev opt-level 1 (test tetap 0).

Benchmark engine memakai salinan tetap DESIGN.md: 27.032 byte, 298 baris, SHA-256 `5f7cc117bb61a4d148122bb121a6bf4c634c2a0fd72af5b3955168cd0aac799b`. Window 28 baris, maju/mundur langkah 3 hingga baris 270, budget 200; waktu proses per window termasuk seluruh slice sampai selesai. Satu pengamatan Linux x86_64, bukan SLA atau percentile.

| Metrik (ms) | Lama: fancy/dev0 | Onig/dev0 | Onig + cache/dev0 | Default baru: Onig + cache/dev1 |
| --- | --- | --- | --- | --- |
| Load SyntaxSet | 86,072 | 97,334 | 92,968 | 3,750 |
| Cold highlight window pertama | 5532,365 | 59,603 | 58,151 | 20,770 |
| Scroll turun maksimum/window | 123,371 | 5,038 | 5,149 | 2,577 |
| Scroll balik maksimum/window | 940,193 | 56,215 | 0,098 | 0,045 |

Biaya CPU renderer dibandingkan setelah perbaikan parser/cache, pada urutan identik 24 wheel turun +24 naik di PTY 120×36. Ini **detik CPU proses**, bukan persentase CPU sistem dan bukan perbandingan langsung terhadap angka 15–20% pengguna.

| Pengukuran | Dev opt0 | Dev opt1 |
| --- | --- | --- |
| CPU untuk 48 langkah scroll | 0,77 s | 0,10 s |

| Verifikasi perilaku | Bukti |
| --- | --- |
| Unit/build | Suite 240 dijalankan: 238 lolos; dua asumsi fixture scope dikoreksi. Rerun final: 18 tes highlight, 3 syntax dan 4 chrome seluruhnya lolos; format/build sukses |
| Cache | Zero-budget scroll balik memulihkan token; batas row/payload, invalidasi edit/path/language/barrier, viewport sparse dan scope multiline diuji tanpa timing assertion atau identitas pointer |
| Markdown nyata | Cold open/48 wheel dua arah pada input benchmark, tanpa perubahan bytes; 324.384-byte Markdown menerima End/Home saat highlight masih pending, PageDown mengikuti 26 baris teks |
| Edit shared | Menghapus delimiter HTML comment memperbarui warna kedua view; save menulis bytes tepat, undo/save mengembalikan sumber. Bantuan tetap mengisolasi command, minimum 12×6 dan restore split terbukti |
| TS/TSX | Fixture asli ter-highlight; fungsi/angka/type pada TS, tag/angka/komentar JSX pada TSX. JS/YAML/HTML/CSS tetap berwarna dan semua fixture tetap utuh; tidak menjalankan React/JS |
| Spacing/input | Overhead normal 9→7 baris, tanpa gap luar; frame tetap. Klik parent pada koordinat baru, focus frame dan helper Help lolos di Light/256/Unicode serta Terminal/none/ASCII; none tanpa SGR warna eksplisit |
| Idle/lifecycle | Sampel idle satu detik tidak menambah CPU tick pada Markdown kecil/panjang. Delapan sesi PTY memulihkan termios/mouse/alternate-screen/bracketed-paste |

Grammar berlisensi beserta pin sumber di `assets/syntaxes/`, notice native di `assets/licenses/Oniguruma.txt`. Dump dibangun lokal dan di-embed, bukan YAML parse/network pada runtime. Satu operasi regex tidak bisa dipotong paksa oleh slice; performa file ekstrem, emulator/font/OS lain dan seluruh kriteria rilis tahap 4 tetap terbuka. Probe/fixture runtime sementara dibersihkan; `testfile` dipertahankan.

### Hasil clipboard sistem dan konfigurasi persisten

Batch menambahkan kenyamanan editor teks, bukan fitur IDE atau session restore. Dua worker implementasi memiliki modul/entry point terpisah; integrator meninjau jalur file/clipboard, menghapus tes wording/forwarding, dan menjalankan verifikasi terkoordinasi.

- Preferensi root TOML `theme`, `color`, `icons`, `clipboard`: lokasi XDG/HOME absolut, maksimum 64 KiB UTF-8, tidak ada project config. Prioritas bawaan < file < CLI; `--save-config` menyimpan lalu keluar, bukan membuka editor atau menyimpan buffer.
- Clipboard Auto memilih Wayland (dua helper), lalu X11; tanpa helper atau pada lingkungan SSH memakai internal. System eksplisit melaporkan kegagalan. Tidak ada akses saat render/idle/seleksi, tidak ada shell/OSC 52. Copy Wayland menyebut MIME UTF-8 agar helper tidak menebak tipe isi.
- Paste eksternal dibatasi 1 MiB dan validasi UTF-8 utuh; read gagal/kosong tidak menyisipkan data internal lama atau menghapus seleksi. Cut gagal tidak menghapus sumber. Deadline helper 2 detik, cleanup grup proses dan reap grace 250 ms; owner clipboard yang sukses dipertahankan.

| Verifikasi | Bukti yang dijalankan |
| --- | --- |
| Format/unit/build | Format dijalankan; 7 tes config, 6 clipboard, 60 app, 3 parser CLI dan parser tema lolos melalui filter terarah (beberapa filter overlap). Rerun 6 clipboard setelah MIME eksplisit dan build akhir tanpa warning. Suite lengkap 260 tes tidak diklaim dijalankan |
| Preferensi nyata | CLI save/merge menghasilkan file 0600 dan direktori baru 0700; PTY normal tanpa flag memakai Nerd tersimpan. Override ASCII/none tidak mengubah bytes konfigurasi |
| X11 nyata | Xvfb terisolasi + xclip: Ctrl+C mengirim Unicode/newline tepat; external Ctrl+V CRLF dinormalisasi, shared panes, save/undo dari panel lain, cut/save/undo dan resize 12×6→120×36 menjaga bytes sumber |
| Wayland nyata | Sway headless + wl-clipboard: Auto memilih Wayland meski X11 juga tersedia; copy/paste editor, Find/Replace, replace/save/undo, Open path dan isolasi Help terbukti. Owner melayani clipboard sesudah aplikasi keluar |
| Ingress negatif | Native xclip menyajikan UTF-8 invalid, 1 MiB+1 dan data kosong; paste/save mempertahankan seluruh bytes fixture dan seleksi tidak diganti data lama. PTY diperlebar untuk membaca alasan status yang terpotong pada 120 kolom |
| Helper macet | Server X11 milik fixture sengaja di-pause; cut gagal setelah pengamatan 2,026 detik, file tetap utuh, helper sudah di-reap. Server di-resume sebelum cleanup; angka satu skenario, bukan SLA |
| Fallback/privasi | Tanpa helper, internal copy/paste/save/undo tetap berfungsi. System tanpa helper gagal tanpa menghapus sumber. Lingkungan SSH disimulasikan dengan dua display nyata: Auto tetap internal dan clipboard keduanya tidak berubah; bukan pengujian SSH jarak jauh |
| Config gagal/startup | Default rusak gagal sebelum raw mode; save tidak menimpa file rusak. Help/version independen, `--no-config` dan probe berjalan pada PTY. Key berisi ESC tidak mengirim kontrol terminal; FIFO ditolak; explicit config dapat dibuat lewat save |
| Lifecycle | 11 sesi PTY termasuk percobaan awal Wayland dan probe keluar 0; termios, alternate screen, bracketed paste dan mouse dipulihkan. Seluruh fixture source dikembalikan ke bytes awal |

Percobaan awal Wayland membaca clipboard sebelum copy selesai; setelah menunggu perubahan state, bytes tepat. Driver juga semula menunggu substring error yang terpotong oleh lebar status; memperlebar PTY menampilkan alasan. Keduanya keterbatasan driver pembuktian, bukan klaim kehilangan data. Paket/backend/font/config sistem pengguna tidak diubah; paket unduhan, compositor/server dan fixture sementara dibersihkan.

Batas tersisa: helper desktop harus tersedia pada PATH absolut; tidak diinstal otomatis. Config menolak target save symlink/hard link/non-regular, bukan mengedit target link. Tidak ada preferences dialog, OSC 52, session persistence atau crash recovery; backlog session tetap berlaku. Matriks terminal/font/emulator/SSH/OS universal dan audit dependency menyeluruh tetap terbuka.

### Hasil status panjang dan profil dokumen besar

Batch ini memperbaiki renderer status dan **mengukur**, bukan mengklaim telah mengoptimalkan seluruh dokumen besar. Batas load/save 64 MiB dan batas highlight per baris tidak diubah. Dua slice independen: worker renderer dan pengumpulan baseline runtime; integrator mengoreksi metodologi, meninjau kode, serta memverifikasi binary akhir.

Status tidak lagi memberi pesan hanya setengah anggaran demi path. Copy/Cut/Paste failed dikenali sebagai error; error/warning mengalahkan path dan metadata opsional, dengan elisi tengah grapheme-safe yang mempertahankan aksi serta akhir penyebab. Head/tail ditulis dari borrowed slices; perhitungan kolom cursor dilewati jika metadata pasti tak akan tampil. Dock/helper, hit target dan API Screen/Geometry tidak berubah.

#### Metode dan hasil binary akhir

Linux x86_64, profil dev opt-level 1, PTY 120×36, ASCII/tanpa warna, clipboard internal, tanpa konfigurasi pengguna. Binary SHA-256 `418dfa9c237bb11578e64663e990328018fa0fafa5a92e090b7ebc4ab583065f`. Cache filesystem tidak dikosongkan; satu sesi per fixture pada pass akhir, bukan percentile/SLA atau benchmark cold disk.

Timer teks pertama dimulai **sebelum spawn**, termasuk setup PTY/proses; bukan waktu kompilasi `cargo run`. Latensi input dihitung sampai perubahan teks body/marker terlihat, termasuk overhead decoder/emulator Python, bukan waktu native murni atau seluruh frame fisik selesai. `select` bangun saat bytes tersedia; quiet drain 3 ms berada di luar interval input. Sidebar disembunyikan dengan menunggu perubahan body; 24 PageDown lalu 24 PageUp untuk fixture multiline. EOF, ketik satu karakter, undo, lalu Home diamati terpisah; file tidak disimpan atau dimutasi di disk. RSS/VmHWM berasal dari proses Notefart, bukan Python.

| Fixture | Teks pertama (ms) | Page max (ms) | EOF (ms) | Edit / undo (ms) | RSS awal / VmHWM (MiB) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Plain 1 MiB | 80,2 | 11,8 | 12,0 | 10,8 / 21,5 | 6,6 / 6,8 |
| Plain 16 MiB | 221,5 | 25,7 | 10,4 | 11,2 / 17,5 | 24,1 / 39,1 |
| Plain 64 MiB | 790,0 | 12,1 | 11,9 | 9,9 / 19,4 | 79,4 / 142,5 |
| Markdown 4 MiB | 90,7 | 31,0 | 21,3 | 12,1 / 17,0 | 11,6 / 13,1 |
| Satu baris 16 KiB | 59,6 | — | 17,7 | 19,4 / 29,9 | 5,7 / 5,7 |
| Satu baris 1 MiB | 67,0 | — | 736,8 | 698,6 / 704,7 | 6,8 / 6,9 |

Fixture sintetis LF dengan marker awal/akhir; ASCII long-line berukuran tepat. Ukuran, jumlah baris (termasuk EOF tanpa newline) dan SHA-256 diverifikasi dari bytes setelah pengukuran:

| Fixture | Byte / baris | SHA-256 |
| --- | ---: | --- |
| Plain 1 MiB | 1048576 / 15198 | `3efb08a856e80e105b5c60d1247d98468ac8259e54a998845491b1f6a426d179` |
| Plain 16 MiB | 16777216 / 243149 | `905f53eeb130fe1118c16db9a8b8d0292fd596633c86599e3011a721d51b257a` |
| Plain 64 MiB | 67108864 / 972593 | `e79b198e8ef5817f7f1f2d167a18f020ce5b4d8ef6e6d91084dd68554bf961ac` |
| Markdown 4 MiB | 4194304 / 216945 | `2385c64110853cc6b5d360e58684e774bc170ed5c7a87b212c1d20b63fdd1261` |
| Satu baris 16 KiB | 16384 / 1 | `192b21654f45c0019edc5cd3c6f181d9f1e836ea31b7dd025d8a2b6e1588194c` |
| Satu baris 1 MiB | 1048576 / 1 | `9b45ead6c17f3d68b9616c097f1f44090327df462b581f0ff5248906637d4320` |

#### Temuan dan batas

- **Panjang baris adalah bottleneck terpisah dari ukuran file.** Satu baris ekstrem masih melakukan scan dari awal pada `Document::column_of` dan iterasi sel sebelum viewport horizontal di renderer. Hasil di atas tidak menjanjikan editing baris ekstrem cepat; optimasi geometri/cursor menjadi pekerjaan berikutnya, bukan diganti plaintext untuk menyembunyikan masalah.
- **Teks terlihat bukan highlight selesai.** Pada sesi tambahan binary akhir, Markdown di-jump ke EOF lalu diamati selama 20 detik. CPU proses bertambah 4,14 detik, RSS akhir 11,69 MiB dan VmHWM 13,23 MiB; Home tetap mendapat acknowledgement sekitar 41,9 ms. Ini observasi beban background, bukan bukti parser sudah menyelesaikan semua prefix atau persentase CPU seluruh mesin.
- RAM tabel adalah jendela skenario tersebut, bukan batas RSS seluruh history/Replace All atau kebocoran memori yang sudah dibuktikan tidak ada. Belum mencakup seluruh Unicode/grapheme, filesystem lambat, terminal/font fisik, SSH atau profil warna.
- Driver awal mengukur setelah spawn/hash binary dan menunggu jendela baca 50 ms; angka latensinya tidak dipakai sebagai hasil akhir. Pass koreksi dimulai sebelum spawn dan menunggu perubahan nyata; pass akhir memakai binary sesudah perbaikan UI.

#### Bukti renderer dan runtime

- `cargo fmt --all`, `cargo test --bin notefart ui::tests`: **29 lolos**, termasuk lima regresi status untuk prioritas, style, elisi dua ujung, profil ikon dan input Unicode/kontrol. Build akhir tanpa warning. Suite lengkap 265 tes tidak diklaim dijalankan.
- Pada TUI 120 kolom, pesan Copy/Cut/Paste gagal menampilkan backend/penyebab tanpa digantikan path; source fixture tetap utuh. Warning F5 dan warning highlight 16 KiB tampil utuh dengan style warning. Elisi pesan/path panjang diuji melalui renderer dengan data error/warning panjang, bukan mengarang bentuk pesan F5.
- Profil aktual: ASCII/none, Unicode/truecolor, Light/256/Nerd serta Unicode/256. Resize 12×6→120×36 dan Help kembali ke warning terbukti. Driver menunggu acknowledgement setiap ukuran; resize cepat tanpa menunggu sempat mengacaukan state emulator, bukan bukti kehilangan state aplikasi.
- **18 sesi PTY integrator**, mencakup baseline terkoreksi, pass akhir, observasi background dan surface status, seluruhnya keluar 0 serta memulihkan termios/alternate screen/mouse/bracketed paste. Enam sesi baseline worker terpisah tidak dipakai untuk angka latensi final. Fixture, capture dan script sementara dibersihkan; hasil yang dipertahankan ada di bagian ini.

### Hasil optimasi lookahead tabel Markdown

Profiling native menunjuk regex batas blok yang mengandung ekspresi tabel: 146,0 ms dari 366,0 ms pencarian regex pada prefix sintetis 8.192 baris. Bagian mahal adalah percobaan balanced-cell pada paragraf tanpa pipe, bukan pemetaan scope. Eksperimen guard awalan karakter pada aturan batas blok lain tidak memberi manfaat dan tidak dipertahankan.

`build_support/markdown.rs` menambahkan lookahead perlu-ada-pipe tepat sebelum ekspresi tabel yang sudah diekspansi Syntect. Kedua cabang aturan asli membutuhkan pipe literal; seluruh aturan/capture/context asli tetap dijalankan jika ada pipe. Dua pemakaian ekspresi diperbarui saat build, tanpa parser baru, cache tak terbatas, dependency baru atau pemotongan fitur highlight. Perubahan grammar upstream yang tidak dikenali menggagalkan build agar invariant ditinjau, bukan diam-diam mengoptimalkan aturan berbeda.

#### Pengukuran lokal

Linux x86_64, dev opt-level 1, PTY 120×36, TERM=xterm-256color, tanpa config pengguna; warna aktif. Notefart memakai profil warna auto/ikon Unicode dan clipboard internal karena tidak ada display lokal. GNU nano 8.4 memakai binary distro dan hanya `/usr/share/nano/markdown.nanorc` melalui rc sementara, dengan nomor baris. Viewport teks Notefart 26 baris, nano 32 baris; fitur/grammar dan profil build keduanya tidak identik. Ini perbandingan pengalaman lokal, bukan benchmark engine setara atau janji lintas mesin.

Fixture pembanding: UTF-8/LF sintetis 65.512 byte, 5.063 baris, SHA-256 `ee30590e87242a881d1baaf3e7b98a2c1ec09d85ae292462148e7557ecb722b2`. Isinya mengulang heading, paragraf bold/italic/link/inline code, quote, list, fenced code dan HTML comment. Awal/akhir diberi marker. Prefix pertama 65.500 karakter dipotong pada newline terakhir, kemudian ditambah newline kosong dan marker akhir.

Timer frame pertama dimulai sebelum spawn, selesai ketika marker awal dan footer tampil. Input menunggu perubahan marker body, bukan sekadar label UI; quiet drain 3 ms di luar timer. CPU berasal dari utime+stime proses (`/proc`, resolusi 10 ms), bukan CPU Python atau seluruh mesin. Catch-up setelah frame EOF dihitung sampai dua observasi 200 ms berturut-turut tanpa pertambahan CPU; angka wall mencakup kira-kira 400 ms konfirmasi idle. Satu pass, filesystem cache tidak dikosongkan; bukan percentile/SLA.

| Ukuran | Notefart sebelum | Notefart sesudah | GNU nano |
| --- | ---: | ---: | ---: |
| Frame pertama (ms) | 74,0 | 90,4 | 53,6 |
| Respons EOF (ms) | 20,9 | 38,1 | 22,5 |
| Catch-up sesudah frame EOF, termasuk konfirmasi idle (s) | 4,26 | 3,04 | 0,40 |
| CPU catch-up (s) | 0,80 | 0,56 | 0,00 |
| Respons Home (ms) | 35,4 | 37,9 | 15,9 |
| VmHWM proses (MiB) | 6,99 | 7,11 | 4,44 |

CPU catch-up turun sekitar 30%; tidak ada klaim startup/input/RAM membaik. Angka CPU nano 0,00 berarti tidak ada tick tambahan teramati pada fase sesudah frame EOF, bukan kerja highlight gratis. Nano masih lebih ringan; rules `.nanorc` tidak disalin ke aplikasi.

Binary sebelum SHA-256 `418dfa9c237bb11578e64663e990328018fa0fafa5a92e090b7ebc4ab583065f`; sesudah `e55b14161f37ae5e3e7df8741450fc950bb7a44dd192a8a28caaee2078d55601`. Benchmark engine tight-loop prefix 8.192 baris, budget 200: 470,4→355,6 ms (sekitar 24% lebih singkat). Ini terpisah dari event-loop/idle TUI dan waktu kompilasi.

Prefix tersebut berasal dari fixture 4 MiB/324.304 baris, SHA-256 `4a0f1998f84980ca024296fc79933d0617a8ab14898a43f595e90459d8b62001`, berbeda dari fixture batch sebelumnya. Observasi EOF baseline file penuh belum mencapai CPU-idle dalam deadline 180 detik dan ditutup normal; bukan bukti parser hang atau selesai. Pengukuran completion yang dipakai di tabel adalah fixture kecil identik pada ketiga editor, bukan mengekstrapolasi hasil ke file penuh.

#### Bukti perilaku dan batas

- `rustfmt` pada file Rust berubah, `cargo test syntax::tests -- --nocapture`: **4 lolos**, lalu `cargo build` tanpa warning. Regresi membandingkan scope operations grammar asli dengan hasil build pada tabel berawalan teks/pipe/markup, pipe escaped/inline code, batas blok, Unicode whitespace, CRLF, comment multiline, code fence dan EOF tanpa newline. Tiga tes TS/TSX tetap lolos; suite lengkap tidak dijalankan.
- TUI truecolor membuktikan comment berwarna; menghapus `-->` mengubah `after_comment` menjadi style comment, tiga undo memulihkan style/terminator. Bytes fixture disk tidak berubah.
- Notefart PageDown/PageUp teramati 15,0–41,8 ms, wheel 16,2 ms; nano PageDown/PageUp 10,0/9,3 ms. Angka ini acknowledgement lokal dengan tinggi viewport berbeda, bukan klaim throughput scroll setara.
- Enam sesi PTY integrator keluar 0 dan termios/alternate screen dipulihkan; lima sesi Notefart juga memulihkan mouse/bracketed paste. Seluruh fixture/probe dan salinan dependency instrumentasi sementara dibersihkan; config/font sistem tidak diubah.
- Prefix menuju viewport jauh masih harus diparse untuk state multiline. Persentase CPU saat catch-up tidak dijamin turun, meski jumlah kerja/waktu sampai idle berkurang pada fixture ini. Slice 4 ms tetap kooperatif, bukan batas keras satu regex; batas RAM/CPU proses tidak ditambahkan. File ekstrem, input adversarial, terminal fisik dan platform lain belum dinyatakan tuntas.
- Geometri baris ekstrem, session restore dan installer tidak diubah oleh batch ini.

### Hasil perbaikan pemetaan warna Markdown

Laporan pengguna setelah optimasi menunjukkan Markdown masih tampak plaintext. Penyebab berbeda dari biaya regex: grammar menghasilkan `markup.heading`, `markup.bold`, `markup.italic`, `markup.raw`, `meta.link` dan scope markup lain, tetapi classifier hanya memetakan kategori bahasa pemrograman. Pemeriksaan batch sebelumnya atas warna comment tidak cukup untuk menyatakan warna markup Markdown lengkap.

- Tambah token heading, emphasis, code, link, quote dan marker dengan palet semantik yang sudah ada. Scope nested menggunakan pemilihan match paling spesifik; paragraf/list body biasa tetap Plain. Tidak ada parser/regex tambahan, perubahan geometri atau alokasi per-frame baru.
- `rustfmt` file berubah, **19 tes highlighter + 7 tema + 4 syntax lolos**, kemudian build dev tanpa warning; suite lengkap 267 tes tidak dijalankan. Regresi mencakup kategori markup aktual, offset Unicode, quote/emphasis/code bersarang, comment/raw fence yang bukan heading, plain text, recent cache, invalidasi dan undo. Tes kontras/degradasi warna mencakup token baru.
- Lima sesi PTY integrator: `AGENTS.md` nyata menampilkan heading berwarna dan mempertahankannya setelah PageDown/Home. Fixture membuktikan heading biru, emphasis ungu, code hijau, link/quote teal serta marker list kuning pada Dark/truecolor; teks biasa tidak ikut diwarnai.
- Menghapus penutup code fence mengubah heading di bawahnya menjadi raw-code hijau; tiga undo mengembalikannya menjadi biru. Seleksi mengalahkan warna heading. Light/256 dan Terminal/16 tetap berbeda warna; none tetap default foreground.
- Lima sesi keluar 0 dan memulihkan termios, alternate screen, mouse serta bracketed paste. Fixture disk tetap utuh; probe scope dan direktori sementara dibersihkan, config/font sistem tidak diubah.
- Ini syntax coloring, bukan preview Markdown. Code fence tidak melakukan embedded highlighting per bahasa. Heading Setext mewarnai underline sesuai grammar, bukan judul baris sebelumnya secara retroaktif. Detail pemetaan di [DESIGN.md](DESIGN.md#tema-dan-token-warna); tidak ada klaim benchmark performa baru.

### Hasil geometri sparse untuk baris panjang

Menangani bottleneck geometri yang diukur pada batch dokumen besar, bukan mengganti format/engine teks atau mematikan highlight. Dua worker memiliki `document.rs` dan integrasi/regresi `ui.rs`; integrator meninjau hasil, memperbaiki kasus regional-indicator, menjalankan pemeriksaan terkoordinasi dan mengukur TUI.

Cache turunan menyimpan paling banyak 8 baris panjang; checkpoint byte/char/kolom pada batas grapheme sekitar setiap 1024 char dan frontier terakhir. Baris pendek tetap streaming tanpa mengusir cache panjang. Lookup char/kolom memilih checkpoint, renderer melanjutkan iterator dekat viewport dengan clipping absolut tetap. Edit/undo/redo memotong suffix dari grapheme sebelumnya sebelum Rope berubah, membuang baris sesudahnya dan mempertahankan prefix aman. Clone tidak menyalin cache. Tidak ada dense map, salinan seluruh baris, dependency atau thread baru.

#### Metode dan angka lokal

Linux x86_64, dev opt-level 1, PTY 120×36, ASCII/none, clipboard internal dan tanpa config pengguna. Timer awal sebelum spawn; acknowledgement input sampai marker body berubah, bukan frame fisik selesai. Quiet drain 3 ms berada di luar interval input; CPU utime+stime dari `/proc`, resolusi 10 ms; VmHWM proses editor. Satu pass per fixture/binary, filesystem cache tidak dikosongkan; bukan percentile atau SLA.

Kedua fixture satu baris tanpa newline, marker `LONG_BEGIN`/`LONG_END`, UTF-8 valid:

| Fixture | Byte / char | SHA-256 |
| --- | ---: | --- |
| ASCII | 1048576 / 1048576 | `be9c817d7eb993d3a13b9398dad41f1c3d7b89d7dd9bf26b86b3d9996ab31a0e` |
| Unicode/tab/kontrol | 528340 / 224340 | `446e250446d3a9b742142d1568f2a0e70416e2aceae77666b98f99f286736254` |

ASCII diisi `a` di antara marker. Fixture campuran: marker awal dan 160 `x`, 16.000 pengulangan `ab`, tab, `世`, `e`+combining acute, `👩‍💻`, ESC, zero-width space, `🇮🇩`, space, lalu 160 `z` dan marker akhir. Window observasi ASCII sengaja menjaga emulator Python dari masalah orphan wide-cell/ZWJ; prefix yang dihitung tetap campuran Unicode. Kolom EOF 1-based diverifikasi dengan aritmetika fixture: ASCII 1048577, campuran 384342, sama sebelum/sesudah.

| Fixture / aksi | Sebelum (ms) | Sesudah (ms) |
| --- | ---: | ---: |
| ASCII — frame pertama | 46,7 | 57,1 |
| ASCII — End dingin | 729,7 | 194,8 |
| ASCII — ketik di EOF | 727,7 | 11,8 |
| ASCII — undo | 721,8 | 26,1 |
| ASCII — klik mouse dekat EOF | 716,1 | 1,9 |
| ASCII — Home | 11,1 | 11,0 |
| Campuran — frame pertama | 51,2 | 40,7 |
| Campuran — End dingin | 255,6 | 76,9 |
| Campuran — ketik di EOF | 239,8 | 10,4 |
| Campuran — undo | 244,9 | 20,2 |
| Campuran — klik mouse dekat EOF | 230,1 | 1,6 |
| Campuran — Home | 10,9 | 10,9 |

| Fixture | CPU End sebelum (s) | CPU End sesudah (s) | VmHWM sebelum (MiB) | VmHWM sesudah (MiB) |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 0,70 | 0,19 | 6,93 | 6,88 |
| Campuran | 0,24 | 0,06 | 6,12 | 6,16 |

Binary sebelum SHA-256 `fbc1ffa669c47d907378071a59ab2311e2f679ec78aedc47808cc1017c770844`; sesudah `effce2616fac8560d3d2d49c580cfae4dd16ca02a47b3bb99dbc37f6eb101efd`. Angka RAM hanya jendela skenario ini, bukan bukti penurunan RAM umum atau batas RSS/history.

#### Bukti correctness dan batas

- `rustfmt` file berubah, `cargo test document::tests`: **16 lolos**, `cargo test ui::tests`: **31 lolos**, lalu build tanpa warning. Suite lengkap 271 tes tidak dijalankan. Regresi membandingkan pemetaan cached/resumed dengan sel penuh, serta batas sel streaming dengan segmentasi string utuh; mencakup tab, CJK, combining, ZWJ, Prepend, RI, kontrol/Bidi, CRLF, batas checkpoint/chunk, mutasi, line join/split, clone dan undo/redo.
- Pemeriksaan awal menemukan pairing RI streaming lintas chunk tidak sama dengan string utuh. Inisialisasi checkpoint saja belum cukup; transisi chunk kini membentuk ulang lookbehind sebelum melanjutkan, dengan pre-context tepat sampai offset permintaan. Tes yang gagal lalu lolos setelah perbaikan. Probe sementara juga membandingkan Next/Previous dengan batas grapheme utuh pada fixture tersebut tanpa mismatch.
- Renderer nyata via TestBackend membuktikan viewport di dalam wide glyph, tab dan escape, dua posisi scroll shared yang independen, fase tab, highlight/selection, EOL dan hardware cursor. Koreksi integrasi membuang asumsi keliru bahwa baris pendek tetap terlihat saat seluruh view di-scroll horizontal.
- PTY final mengetik/undo di EOF serta klik/ketik/undo dekat marker dan memeriksa bytes disk tetap. Shared view membuktikan satu pane di Home dan satu di End, edit/save dari satu pane lalu undo/save dari pane lain menghasilkan bytes tepat. Enter menciptakan baris baru/kolom awal; undo mengembalikan kolom akhir, lalu resize 12×6→120×36 mempertahankan kedua view.
- Delapan sesi PTY integrator, termasuk percobaan driver yang dikoreksi, keluar 0 dan memulihkan termios/alternate screen/mouse/bracketed paste. Driver awal salah mengecualikan baris body pertama, percobaan Unicode mengekspos keterbatasan pyte, dan marker pada lebar minimum hanya muat bagian akhirnya; angka tabel berasal dari pass dengan selector/fixture yang dikoreksi. Probe dan fixture sementara dibersihkan, font/config sistem tidak diubah.
- Lookup dingin dan cache miss setelah eviction tetap perlu menelusuri prefix. Lookbehind grapheme tertentu, misalnya run RI panjang, dapat mahal; tidak ada kuota CPU/RAM atau janji semua bentuk baris cepat. Cache 8 baris adalah tradeoff memori/reuse, bukan indeks penuh. Word navigation/seleksi satu kata ekstrem, seluruh terminal/font/OS dan semua ukuran file belum dibenchmark. Soft wrap, session restore dan installer tidak diubah.

### Hasil persiapan CI Linux dan regresi lengkap

- Tambah `.github/workflows/ci.yml`: push seluruh branch, pull request dan manual; Ubuntu 24.04 x86_64, timeout job 30 menit, cancel run lama pada ref yang sama. Cek format → seluruh target tes → build release → smoke CLI → package/upload hanya setelah sukses.
- `rust-toolchain.toml` mem-pin 1.99.1/minimal/rustfmt. Ketersediaan pin diverifikasi lewat manifest distribusi resmi; auto-install pin berhasil lokal tanpa mengubah default global. Cargo tes/build memakai `--locked`.
- Checkout v7.0.1 dan upload-artifact v7.0.2 dipin full SHA hasil pemeriksaan ref rilis resmi. Permission `contents: read`, credential checkout tidak dipertahankan, tidak ada deploy secret, auto-release atau installer.
- `cargo fmt --all -- --check` lolos; `cargo test --locked --all-targets`: **271 tes lolos, tiga target suite**. Ini menutup celah bukti suite lengkap dari batch sebelumnya, bukan audit keamanan atau pembuktian platform universal. `cargo build --locked --release --bin notefart` serta `--version`/`--help` sukses.
- actionlint 1.7.12 lolos; binary validator sementara diverifikasi SHA-256 dari metadata rilis resminya sebelum dijalankan. Command packaging dieksekusi dari definisi YAML: tar berisi tepat binary, `config.toml.example`, Oniguruma dan Sublime Packages notices; bytes sesuai sumber dan executable mode 0755. Perilaku ignore config/.env/target serta pengecualian template dibuktikan tanpa membuat credential.
- Artifact di `target/dist/notefart-linux-x86_64.tar.gz` tersedia dari pembuktian lokal. Workflow mengunggahnya dengan nama yang menyertakan commit SHA dan retensi 7 hari; privasi mengikuti repository. Run Ubuntu, upload/download artifact dan akses akun GitHub **belum diuji**. Lokal tidak memiliki remote; `gh` tidak tersedia. Tidak membuat commit, repo remote, push atau mengubah visibilitas akun/repo.
- Validator unduhan sementara dibersihkan; binary/archive di `target/` adalah hasil build yang di-ignore. Installer/setup config tetap deferred sampai publish; review keamanan/dependency dan matriks terminal nyata tetap terbuka. Metode operasional/pin resmi ada di [README.md](README.md#ci-dan-artifact-linux).

### Hasil migrasi executable naf

- Tambah target `[[bin]]` bernama `naf`, entry `src/main.rs`; package Cargo tetap `notefart`, branding UI tetap Notefart dan config pengguna tetap di namespace `notefart`. Cargo metadata membuktikan hanya satu binary target, `naf`, tanpa shim/alias lama.
- Usage, version dan seluruh prefix error CLI memakai `naf`. Routing startup tidak diubah: tanpa argumen editor kosong, satu path relatif membuka file/folder, path file baru tidak ditulis sebelum Save. `cargo run -- --version` memilih `target/debug/naf`.
- Workflow build/smoke/package/upload serta panduan aktif memakai `naf`; artifact menjadi `naf-linux-x86_64.tar.gz` dengan executable `naf`. Output lama bernama `notefart` pada debug/release/dist dibersihkan, bukan mempertahankan binary usang yang dapat dipanggil keliru.
- Format check lolos; tiga tes CLI yang sudah ada lolos melalui filter exact; build dev dan release `--locked --bin naf` sukses. Release help/version dan prefix error juga diverifikasi. Tidak menambah tes wiring/nama atau menjalankan ulang suite luas; bukti suite lengkap batch CI sebelumnya tetap historis.
- Tiga sesi TUI menjalankan nama `naf` melalui PATH sementara khusus driver, tanpa flag tambahan: editor Untitled bisa diketik/undo; `naf test.txt` membuka path relatif serta edit/save/undo memulihkan bytes sumber; `naf new.txt` tidak membuat file sampai Save. Semuanya keluar 0 dan memulihkan termios/alternate screen/mouse/bracketed paste. Ini bukan pemasangan binary ke PATH pengguna.
- Command packaging dari YAML final dijalankan lokal: arsip memuat tepat `naf`, template dan dua notice, bytes binary sesuai release dan mode 0755. Upload/run GitHub belum dijalankan; status remote tidak diubah. Fixture sementara dibersihkan; hasil build baru tetap di `target/`.

## Disiplin verifikasi

- Gunakan unit test untuk invariant dokumen, transaksi, checkpoint, posisi view, dan layout; bukan snapshot wording atau source text.
- Gunakan temporary directory dan fault injection terisolasi untuk I/O; jangan mengubah permission/disk pengguna untuk mengetes kegagalan.
- Buktikan interaksi TUI melalui aplikasi nyata, bukan hanya mock event.
- Tambahkan regression test ketika bug perilaku belum tercakup test yang ada.
- Setelah perubahan terverifikasi, perbarui dokumen terkait dan changelog ketika implementasi mulai tersedia. Hapus spike yang sudah tidak diperlukan.

## Fitur tambahan — session restore

Dicatat atas permintaan pengguna untuk kenyamanan ala Notepad, bukan perluasan menuju IDE. **Belum diimplementasikan**, dan tidak menjadi prasyarat penyelesaian batch clipboard/konfigurasi atau janji rilis.

- Rencana cakupan: membuka kembali file dan urutan tab, panel aktif, layout split, serta posisi cursor/scroll.
- Pemulihan dokumen Untitled dan perubahan belum disimpan juga perlu dirancang agar pengalaman session benar-benar berguna; jangan menganggap menyimpan daftar path sudah melindungi buffer tersebut.
- Sebelum implementasi, tetapkan izin/lokasi snapshot, penanganan snapshot rusak, file hilang/berubah di luar aplikasi, dan privasi isi buffer. Restore tidak boleh menimpa file sumber secara diam-diam.
- Session restore berbeda dari crash recovery. Saat ini keluar tetap menggunakan Save/Discard/Cancel; tidak ada penyimpanan sesi, autosave, atau jaminan pemulihan setelah crash.

## Distribusi — installer saat publish

Installer ditunda sampai tahap publish sesuai keputusan pengguna; belum diimplementasikan. Rencana mencakup setup preferensi dan generate konfigurasi setelah pemasangan, dengan instalasi user tanpa sudo sebagai default. Jika instalasi sistem memerlukan root, tampilkan instruksi hak akses yang jelas; konfigurasi tetap milik pengguna dan file yang sudah ada dipertahankan secara default.

## Belum diprioritaskan

LSP/autocomplete, debugger, Git UI, plugin, terminal tertanam, crash recovery, soft wrap, dan dukungan encoding tambahan perlu keputusan terpisah. Daftar ini bukan janji fitur atau arah menuju IDE; usulan baru harus tetap mendukung produk editor teks.
