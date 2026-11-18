# Roadmap Notefart

Status: draft tanpa tanggal rilis. Urutan di bawah adalah usulan implementasi balanced, bukan pengurangan fitur yang telah disepakati.

Diagnostik, editor aman, workspace, hybrid panel + tab/shared view dan P1 terpilih sudah berjalan. Sesuai keputusan pengguna, tahap 4 ditutup untuk matriks Linux yang telah dibuktikan; bukan klaim seluruh terminal/font/host SSH. Bukti serta batas terkini ada di [hasil P1 aksesibilitas dan pencarian](#hasil-p1-aksesibilitas-dan-pencarian). Publish dan installer tetap terpisah.

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

Status terkini: **ditutup pada cakupan Linux teruji yang disetujui pengguna**; lihat [hasil P1 dan penutupan](#hasil-p1-aksesibilitas-dan-pencarian). Pernyataan gap pada catatan batch lama di bawah adalah kondisi historis, bukan status pengujian terbaru.

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

### Hasil navigasi kata panjang dan validasi editor tmux

Lanjutan non-security tahap 4 menutup celah pengukuran navigasi kata dari batch geometri sparse. Baseline nyata menunjukkan Ctrl+panah pada satu run panjang masih mengulang lookup Rope/grapheme per langkah. `Document::word_left`/`word_right` kini menghitung arah yang dibutuhkan saja; double-click memakai pemindaian yang sama untuk kedua ujung seleksi. Iterator char berjalan berurutan, perubahan kelas hanya memutus run pada boundary grapheme. Tidak ada penyalinan teks, indeks/cache baru, perubahan kelas kata atau dependency tambahan.

#### Pengukuran lokal

Linux x86_64, dev opt-level 1, tmux 3.5a dengan server/socket terisolasi, `TERM=tmux-256color`, viewport 120×36, ASCII/tanpa warna, clipboard internal dan tanpa konfigurasi pengguna. Satu pass per fixture/binary, filesystem cache tidak dikosongkan. Timer dimulai sebelum `tmux send-keys` sampai `capture-pane` menampilkan posisi cursor yang dituju; overhead subprocess/pengamatan termasuk. Ini bukan percentile, waktu native murni, frame fisik atau SLA.

Fixture ASCII: `HEAD ` + 1.048.576 karakter `a` + ` TAIL\nNEXT` (1.048.591 byte). Fixture Unicode: `HEAD ` + 32.768 pengulangan `e` + combining acute + `世_` + ` TAIL\nNEXT` (229.391 byte). Fixture spasi: `HEAD` + 1.048.576 spasi + `TAIL\nNEXT` (1.048.589 byte). Pada fixture kata, satu Ctrl+Right awal menuju awal kata panjang sebelum interval ukur. Ctrl+Right terukur melintasi run dan mengisi geometri dingin menuju `TAIL`; Ctrl+Left berikutnya kembali dengan cache geometri yang sudah hangat.

| Fixture / aksi | Sebelum (ms) | Sesudah (ms) |
| --- | ---: | ---: |
| Kata ASCII — Ctrl+Right | 1745,4 | 257,3 |
| Kata ASCII — Ctrl+Left | 1525,6 | 46,2 |
| Kata Unicode — Ctrl+Right | 248,3 | 100,2 |
| Kata Unicode — Ctrl+Left | 235,2 | 65,9 |
| Spasi — Ctrl+Right | 1742,9 | 229,4 |
| Spasi — Ctrl+Left | 2606,1 | 40,5 |

SHA-256 binary sebelum `fc83f0b67db6be170b624a92e2354beffa77b952bd3d38215896f3063e2166e2`; sesudah `025418dd9ea111c5b197ced40fac94f8dcc2e9730b2de4d997bc8c959f76e9b6`. Kedua binary memakai resep fixture dan urutan input identik. Bytes fixture tidak berubah dan seluruh sesi ukur keluar status 0. Tidak ada klaim startup/RAM umum membaik.

#### Bukti perilaku dan batas

- `rustfmt` pada file Rust berubah, `cargo test --locked --bin naf document::tests`: **19 lolos**, lalu `cargo build --locked --bin naf` sukses. Tiga regresi baru mencakup kelas karakter pertama grapheme, combining/Prepend/ZWJ/RI, whitespace lintas CRLF/baris kosong, run lintas chunk, seleksi dan endpoint navigasi. Suite lengkap tidak dijalankan; ini bukan security test.
- Empat sesi editor tmux: Dark/truecolor/Unicode, Light/256/Unicode, Terminal/none/ASCII dan Dark/16/Nerd. Ctrl+panah serta Ctrl+Shift+panah memilih kata Unicode; paste mengganti seleksi tepat. F8/F9 menghasilkan tiga view shared; undo/redo dan save dari view lain menghasilkan bytes yang diharapkan.
- Resize 120×36→20×6 menyisakan view aktif, seleksi/paste/save saat compact tetap berlaku; kembali 120×36 memulihkan tiga view dengan teks terbaru. F12 menutup satu panel tanpa membuang dokumen shared. Double-click SGR pada kata memilih hanya kata tersebut, penggantian dapat di-undo dari panel lain, lalu seluruh bytes sumber dipulihkan.
- Frame split dan compact diamati melalui `capture-pane`; empat sesi keluar 0, termios sebelum/sesudah identik dan output menonaktifkan alternate screen, mouse serta bracketed paste. Seluruh fixture, driver, salinan baseline dan server tmux sementara dibersihkan; konfigurasi/font pengguna tidak diubah.
- Pemindaian tetap perlu membaca run yang dilintasi; boundary Unicode tertentu memerlukan lookbehind dan geometri dingin masih memiliki biaya. Tidak ada indeks seluruh dokumen, batas keras CPU/RSS atau janji semua file hingga 64 MiB cepat.
- Sesi validasi tidak memiliki display yang terpasang; executable emulator grafis dan `sshd` tidak ditemukan di PATH. Input tmux memakai `send-keys`/paste buffer dan injeksi protokol mouse, bukan keyboard/klik manusia atau validasi shortcut client tmux. Matriks font/emulator/SSH, evaluasi visual final dan kriteria rilis tahap 4 tetap terbuka. Session restore tetap fitur tambahan; installer tetap ditunda sampai publish.

### Hasil session restore otomatis

Pengguna memilih aktivasi otomatis pada `naf` tanpa path. Implementasi menambahkan `session` untuk I/O, ekspor/impor workspace, rekonstruksi split tree dan integrasi startup/quit. Tab, panel aktif, urutan/ratio split, cursor/selection/viewport serta buffer Untitled/dirty dipulihkan; satu dokumen shared tetap satu teks/history. Baseline disk asli dan format/read-only disimpan, sedangkan undo/redo dimulai ulang.

- `rustfmt` pada file berubah; `cargo test --locked --bin naf restored_dirty_checkpoint`: **1 lolos**; `cargo test --locked --bin naf cli_`: **4 lolos** (CLI/config/theme), lalu `cargo build --locked --bin naf` sukses tanpa warning. Regresi baru membuktikan undo setelah restore dirty tidak mengaku bersih sebelum Save eksplisit. Suite luas tidak dijalankan.
- Lima sesi TUI tmux 3.5a, `TERM=tmux-256color`, ASCII/tanpa warna/clipboard internal dan konfigurasi/state fixture terisolasi. Exit normal lalu relaunch dari CWD berbeda memulihkan body tab/panel/ratio/viewport/fokus identik. Selection yang dipulihkan mengganti teks tepat; shared background tab mempertahankan cursor independen.
- Buffer Untitled dapat di-Save As setelah restore; dirty source tidak berubah di disk sampai Ctrl+S eksplisit, lalu BOM/CRLF dan bytes yang diharapkan tepat. Close tab dirty tetap meminta Save/Discard/Cancel; Discard tidak muncul lagi pada snapshot normal-exit berikutnya.
- `--no-session` mempertahankan quit guard dan launch file langsung membuka file yang diminta. Keduanya tidak mengubah snapshot default. Resize 120×36→20×6 menunggu acknowledgement chrome compact aplikasi, lalu kembali 120×36 memulihkan panel. Lima sesi pass akhir keluar 0, termios identik sebelum/sesudah dan alternate screen/mouse/bracketed paste dipulihkan.
- CLI `--help` diamati pada binary baru. SHA-256 binary dev `53da2e1676267c20e77bd0e0a2a006d5324791bcde0240566d982dab892afa3c`. Driver awal dikoreksi untuk viewport nonaktif setelah split, menunggu dialog hilang setelah Esc, dan acknowledgement renderer saat resize; tidak mengubah produk untuk meluluskan asumsi driver.
- Snapshot memakai format v1 NAFSES01, metadata JSON 4 MiB, teks 64 MiB/dokumen, total 256 MiB dan batas 1024 dokumen/256 panel/4096 view. Lokasi/privasi serta semantik sumber berubah/hilang ada di [ARCHITECTURE.md](ARCHITECTURE.md#keputusan-session-restore-otomatis) dan [SECURITY.md](SECURITY.md#snapshot-sesi-dan-privasi).
- Security test terarah dan review terpisah diselesaikan model `atmo/alicn/deepseek-v4.1-flash-1017`; integrator memeriksa laporan serta log hasil tanpa menjalankan ulang pengujian tersebut. Tidak ditemukan kerentanan sesi yang terkonfirmasi; cakupan parser, mode/link, lock/isolation, kegagalan save, konflik source dan batasnya ada di [SECURITY.md](SECURITY.md#hasil-verifikasi-model-terpisah).
- Temuan UX pesan `--no-session` ganda diperbaiki dengan satu pemilik konteks di App. Build dev akhir sukses, SHA-256 `9242eb967a5af206c4338a83af34b8270434faadab5da3b644b2d5a0c6e6c53b`; model yang sama mengamati ulang instance kedua ditolak nonblocking dengan saran hanya satu kali, tanpa mengulang suite keamanan.
- Bukti fungsional dan verifikasi keamanan terarah tidak menjanjikan crash recovery, performa snapshot ekstrem, perlindungan dari semua race/filesystem, font/emulator fisik, SSH atau penutupan seluruh tahap 4. Installer tetap ditunda sampai publish.

### Hasil visibilitas explorer berdasarkan startup

- `naf` tanpa path selalu memulai dengan explorer tampil, termasuk restore snapshot yang sebelumnya hidden. Buffer/tab/split, root dan lebar explorer tetap dipulihkan. `--no-session` tanpa path juga menampilkan explorer; argumen folder tampil, argumen file existing maupun baru tersembunyi.
- Ctrl+B yang sudah tersedia dipakai untuk hide/unhide, tanpa shortcut kedua atau flag baru. F2 tetap pindah fokus. Hide/unhide mempertahankan root/lebar/buffer; drag pembatas dan layout compact tetap berlaku.
- `cargo fmt --all -- --check` lolos; `cargo test --locked --bin naf app::tests::explorer_navigation_focus_and_open_flows -- --exact`: **1 lolos**; `cargo build --locked --bin naf` sukses tanpa warning. Help CLI menampilkan aturan startup baru. Tidak menjalankan suite luas atau security test.
- **Tujuh sesi TUI tmux 3.5a**, `TERM=tmux-256color`, ASCII/tanpa warna/clipboard internal, config dan state fixture terisolasi: startup tanpa path; restore sesi hidden dari CWD lain; file existing; file baru; folder; tanpa sesi; dan relaunch untuk lebar tersimpan. Semua menghasilkan visibility yang diminta, exit 0, termios identik dan alternate screen nonaktif setelah keluar.
- Input nyata membuktikan toggle dua arah, hide saat explorer fokus lalu mengetik kembali di editor, drag lebar enam kolom, resize 120×36→20×6→120×36, serta pemulihan lebar/root/buffer saat relaunch. Mouse setelah toggle tetap mengedit posisi yang tepat; Save menulis bytes fixture yang diharapkan dan undo/Save mengembalikan sumber. File baru hanya dibuat saat Save.
- Driver awal mengasumsikan lima karakter ketikan sebagai satu undo; dikoreksi menjadi lima transaksi tanpa mengubah produk. Server tmux dan fixture sementara dibersihkan. Ini bukti protokol/cell, bukan validasi terminal/font fisik atau shortcut client tmux.

### Hasil validasi workflow release pada terminal Orca

- `cargo build --locked --release --bin naf` sukses tanpa warning. `target/release/naf --version` menghasilkan `naf 0.1.0`; SHA-256 binary teruji `3129c03a30c881acb41bda69dedcf8e7a5d941aeac33caf716e7fbd61fe380cf`. Tidak ada perubahan kode produksi pada batch validasi ini.
- Terminal Orca baru dibuat tanpa mengambil fokus atau memakai terminal pengguna. Hasil create menyatakan `surface: background`; observasi memakai `terminal read --screen` dengan `source: screen`, bukan output repaint bertumpuk. Lingkungan aktual: Linux, 120×40, `TERM=xterm-256color`, `COLORTERM=truecolor`, `LANG=en_US.UTF-8`, tanpa tmux.
- **Tiga sesi aplikasi release** memakai fixture/config/state terisolasi dan clipboard internal. Profil yang dijalankan: Dark/truecolor/Unicode serta Light/256/ASCII. Ini membuktikan alur pada backend terminal langsung, bukan screenshot GUI atau validasi warna/font fisik.
- Sesi file: paste UTF-8 CJK/combining, split shared, edit lalu undo/redo dari panel lain, Replace All dan undo/redo lintas panel, serta Save menghasilkan bytes tepat. Ctrl+B/F2 mengembalikan input ke editor sesudah explorer di-hide. Klik pada sel kedua CJK snap ke awal grapheme; edit/undo/Save tetap tepat. Copy seluruh dokumen ke tab baru, paste, Save As dan close menghasilkan salinan Unicode identik.
- Sesi default: buffer Untitled/dirty Unicode dibuka dalam tiga view nested; explorer disembunyikan sebelum Ctrl+Q. Relaunch dari CWD lain memulihkan root, tiga view dan teks, tetapi explorer tampil sesuai aturan startup. Save As setelah restore memperbarui seluruh caption shared; F1 dan Esc mempertahankan field serta pending Save As. Menutup satu panel mempertahankan shared document.
- Ketiga sesi keluar 0 dan termios sebelum/sesudah identik. Receipt close terminal fixture mengonfirmasi `ptyKilled: true`; fixture serta driver sementara dibersihkan. Tidak menyentuh file/config/snapshot pengguna atau terminal lain.
- Percobaan `tui-idle` pada shell biasa timeout; kondisi tersebut tidak dipakai sebagai bukti editor. Setiap aksi berikutnya diperiksa melalui screen aktual dan bytes file, tanpa mengulang input yang sudah diterima.
- Batas: host tidak menyediakan DISPLAY/Wayland maupun emulator grafis; provider Computer Use gagal memuat python3-gi/AT-SPI. Tidak memasang paket desktop atau mengambil fokus pengguna. Glyph/font fisik, shortcut keyboard client, klik mouse fisik, koneksi SSH nyata, platform lain dan matriks penggunaan panjang tetap belum dibuktikan. Tidak menjalankan security test, suite unit luas, packaging, GitHub Actions, publish atau installer; tahap 4 belum ditutup.

### Hasil perbaikan bug P0 audit AGY

- Cakupan: temuan 1.1–1.5, 1.7–1.10, serta kontras foreground pilihan explorer pada 2.1. Status per temuan tercatat di [laporan AGY](E-TO-E_TEST_AGY.md#status-tindak-lanjut-batch-p0); wishlist P1/P2 tidak dinaikkan menjadi fitur.
- Kontrak tetap: Discard berlaku langsung per dokumen, Cancel berikutnya tidak memulihkannya; aturan startup explorer dan `Ctrl+B` mengikuti keputusan pengguna. Dialog menjelaskan konsekuensi Discard.
- Integrator menjalankan `cargo fmt --all`, `cargo build --locked --bin naf`, dan `cargo build --locked --release --bin naf`; semuanya berhasil tanpa warning build. `./target/release/naf --version` menghasilkan `naf 0.1.0`.
- Worker `atmo/alicn/deepseek-v4.1-flash-1017` menjalankan `cargo fmt --all -- --check` dan `cargo test --locked --all-targets`: **293 passed, 0 failed**, serta dua target example tanpa test. Percobaan awal menemukan sembilan assertion lama yang masih memperlakukan `StatusMessage` sebagai string; integrator memperbaiki bagian `cfg(test)` lalu worker mengulang pemeriksaan yang gagal. Tidak ada shim string atau perubahan produksi untuk meluluskan assertion tersebut.
- Review read-only worker atas retry path, kelanjutan Save/Close/Quit, guard Choices kecil, dan overlay Pending/Resumed tidak menemukan kebutuhan perubahan. Worker juga membuktikan tiga kelompok runtime PTY: OSC/DCS dan BEL/ST terfragmentasi, bare SS3/F1, serta paste literal/terfragmentasi dan shortcut nyata setelah paste. Detail dan batasnya ada di [SECURITY](SECURITY.md#verifikasi-input-batch-agy-p0).
- Integrator membuktikan **8 sesi aplikasi TUI non-security** pada fixture terisolasi dan socket tmux sendiri: tujuh debug dan satu release. Skenario meliputi retry Open, retry Save As yang meneruskan Quit, penolakan Discard tersembunyi pada 20×6, notice keluar pada 10×3, canvas compact, status Saved yang tetap terlihat bersama warning highlight terlambat, batas scroll horizontal, serta retry Save As/Quit pada release.
- Runtime warna Light truecolor memperlihatkan border editor aktif RGB `25,92,168` dan canvas compact RGB `245,247,251`. Pada delapan sesi integrator, aplikasi exit 0, termios kembali seperti sebelum launch, dan alternate screen nonaktif setelah keluar.
- Driver smoke dikoreksi agar memilih border editor, menunggu redraw compact/modal selesai, menerima label keluar adaptif, dan tidak menganggap viewport horizontal otomatis direset. Koreksi driver bukan perubahan produk untuk meluluskan pemeriksaan.
- SHA-256 debug: `4ee3da662e8de40d8b526e03346cc6e62eda06d6d74a876c16886a660315822c`; release: `94c0e09a9ac491b9a510e09ddcc66b619975dcc6149e9869f7793fcd81525b52`. Koreksi assertion setelah build hanya menyentuh `cfg(test)`; hash debug tetap sama.
- Cleanup: fixture dan server tmux milik delapan smoke integrator dibersihkan, skrip throwaway worker dihapus, dan terminal worker dilepas setelah laporan diterima. Server runtime worker sudah nonaktif; socket stale milik pengujian dihapus. Enam tautan/path/anchor dokumentasi baru diperiksa dan valid.
- Batas bukti: tmux/PTY lokal, bukan validasi font/GUI terminal fisik, SSH, installer, atau CI. Ambiguitas byte legacy `Alt+]`/OSC tidak dihilangkan; recovery memakai `Ctrl+G`, bukan timeout yang mengeksekusi payload tertunda.

### Hasil background terminal dan tab aksesibel

- Permintaan pengguna mengganti canvas solid dengan background terminal agar efek glass tidak tertutup. Semua palet kini memakai `Color::Reset` untuk canvas tanpa flag/config baru; foreground Dark/Light tetap dapat dipilih sesuai latar, sedangkan Terminal memakai foreground default.
- Tab aktif memakai pasangan `selection`/`fg` untuk seluruh label, ikon, dirty/read-only dan close; marker fokus serta bold tetap ada. ANSI16/Terminal/none menggunakan reverse sebagai pembeda nonwarna. Prefix field fokus memakai pasangan seleksi yang terbaca, bukan canvas default sebagai foreground di atas accent.
- `chrome::pane_areas` menambahkan satu baris kosong sebelum gutter/teks pada panel setinggi minimal 5. Tinggi minimum 4 menghilangkan gap agar editor compact 12×6 tetap menyisakan satu baris teks. Controller memakai snapshot `text_area` yang diterbitkan renderer; klik gap tidak menjadi posisi teks.
- `cargo fmt --all`, **14 tes terarah warna/UI/geometri**, dan `cargo build --locked --release --bin naf` lolos. Compile test awal menemukan import `Color` yang belum ada; diperbaiki melalui code action LSP sebelum tes dijalankan. Build awal juga menemukan accessor `uses_reverse_cues` obsolete; referensi LSP hanya deklarasi, accessor dihapus dan build release akhir selesai tanpa warning.
- Tiga sesi aplikasi release memakai fixture/config/state terisolasi: Dark/truecolor/Nerd, Light/256/Unicode, dan Terminal/none/ASCII. Capture sel/SGR menunjukkan background default pada canvas, highlight tab aktif dan prefix field dengan pasangan foreground yang tepat, serta reverse/bold tanpa warna.
- Sesi utama membuktikan pindah/close tab lewat mouse, dirty marker, klik gap tanpa memindahkan cursor, klik teks pada offset baru, save dengan isi fixture yang diperiksa, split, collapse pada 12×6, lalu restore panel dan spacing. Dua profil tambahan memeriksa fallback warna/fokus dan pemadatan minimum.
- Driver dikoreksi mengikuti prefix field aktual, Undo satu karakter, dan acknowledgement frame compact aplikasi, bukan reflow tmux sesaat. Tidak ada perubahan produksi untuk meluluskan asumsi driver. Ketiga sesi exit 0, termios kembali seperti sebelum launch, alternate screen nonaktif; fixture, server tmux dan socket stale milik pengujian dibersihkan.
- SHA-256 release teruji: `2c58af398ec294a7c3ac40ef495b3dd17f61225d1ca82193b9ffc9ca9eee407c`.
- Batas: bukti terminal/sel lokal, bukan pembuktian compositor glass, glyph/font fisik, client keyboard/mouse, SSH atau CI. Kontras canvas tidak lagi dapat dihitung dari RGB milik aplikasi; tes kontras hanya menjamin pasangan yang memang dikendalikan aplikasi. Tidak menjalankan security test, suite seluruh target, packaging atau installer.

### Hasil perapatan jarak split bertingkat

- Permintaan pengguna: celah atas–bawah tampak terlalu tinggi dibanding kiri–kanan. Split Rows kini tanpa baris kosong tambahan; Columns tetap satu kolom. Kedua panel mempertahankan frame masing-masing; spacing tab-ke-teks dan background terminal tidak diubah.
- Minimum, preflight, layout dan resize memakai tinggi Rows tanpa gap. Hit resize menumpang border bawah subtree atas; koordinat drag/keyboard dikonversi ke span pertama dengan offset satu baris. Border/tab/teks tidak dihapus atau dijadikan satu area hit.
- `cargo fmt --all` dan 17 target tes geometri/UI diperiksa: putaran awal 16 lolos, satu tes collapse masih menganggap tinggi 4 selalu kurang. Batasnya diubah menjadi minimum tree dikurangi satu, lalu target tersebut diulang dan lolos. Invariant coverage kini menghitung pane/gap kolom, sedangkan hit Rows hanya boleh menumpang border bawah. Akurasi resize diuji pada kedua orientasi hingga koordinat `u16`.
- Smoke Nerd awal membuktikan split bertingkat, mouse dan keyboard resize Rows, resize Columns, klik teks/save serta collapse 12×6 dan restore ratio. Frame tersimpan juga memperlihatkan grip Rows tertutup pengisian gap kolom pada perpotongan. Urutan render divider dibalik agar ancestor digambar terakhir, tanpa alokasi atau pass tambahan; regresi border/grip mencakup layout sederhana/bertingkat, tiga profil ikon dan seluruh panel yang dapat fokus. Target regresi diulang dan lolos; build release akhir tanpa warning.
- Dua smoke pada release akhir: Nerd/truecolor membuktikan grip perpotongan terlihat dan dapat di-drag, shortcut resize tetap satu langkah, gap kolom tetap satu, serta klik teks/save tetap tepat. ASCII/none membuktikan F9 memuat dua panel setinggi 4 pada body 8 baris (terminal 12×11), mempertahankan border/grip dan restore saat diperbesar.
- Seluruh tiga sesi memakai fixture/config/state dan server tmux sendiri, exit 0, mengembalikan termios dan menonaktifkan alternate screen. Fixture, server dan socket stale milik batch dibersihkan.
- SHA-256 release akhir: `ad3493a902691aa4d5bc21cd569c06412c26cfb6edb1557ea6f1430f2e9aa151`.
- Batas: verifikasi sel/protokol terminal lokal, bukan jaminan jarak pixel identik pada setiap rasio font, GUI/font fisik, input client atau SSH. Tidak menjalankan security test, suite seluruh target, CI atau installer.

### Hasil border bersama dan grip terpusat

- Laporan pengguna: grip horizontal menempel pada salah satu dari dua border. Percobaan glyph baseline hanya mendekatkan posisi dan tidak dipertahankan. Pengguna memilih satu border bersama, bukan menambah baris celah atau menerima posisi perkiraan.
- Rows kini berbagi satu baris antara border bawah subtree atas dan border atas subtree bawah; grip berada pada centerline yang sama. Minimum/preflight/layout/resize menghitung border sekali. Span sementara memakai `u32` untuk `parent.height + 1`; akurasi kedua orientasi hingga batas `u16` tetap diperiksa.
- Renderer membentuk junction atas/bawah/cross, termasuk kolom yang tidak sejajar. Judul panel digambar sekali setelah stroke divider, dengan budget sebelum grip/junction; pada minimum sempit label teks menggantikan ikon yang tidak muat. Tab, textarea, fokus dan jalur keyboard/mouse tetap terpisah dari shared border.
- `cargo fmt --all`, **19 tes terarah geometri/UI** dan `cargo build --locked --release --bin naf` lolos tanpa warning build. Regresi renderer meliputi 78 frame: tiga profil ikon, empat topologi split, lebar 12/25/60 yang muat, dan seluruh panel fokus. Invariant tiling mengizinkan overlap hanya pada border bersama, bukan konten atau kontrol tab.
- Dua sesi release akhir memakai fixture/config/state dan server tmux terisolasi. Nerd/truecolor membuktikan empat panel, grip tengah, junction yang sejajar maupun tidak, resize Rows/Columns via mouse dan keyboard, judul pada lebar 25, collapse 12×6/restore ratio, serta klik teks/save dengan isi fixture diperiksa. ASCII/none membuktikan dua panel minimum pada 12×10 (body 7, tiap panel 4 dengan satu border bersama), judul tidak menimpa grip, grow dan resize.
- Sel aktual TUI diraster dengan DejaVu Sans Mono lokal: grip midline berada pada satu garis bersama, bukan menggeser glyph menuju celah setengah baris. Ini inspeksi font lokal, bukan screenshot emulator/font client pengguna. Dependency raster hanya alat inspeksi terisolasi, bukan dependency proyek atau perubahan font sistem.
- Kedua sesi exit 0, termios kembali seperti sebelum launch, alternate screen nonaktif. Fixture, server tmux dan socket stale milik pengujian dibersihkan; percobaan baseline sebelumnya juga sudah ditutup.
- SHA-256 release akhir: `fd45421fdff12bc663b775748fa94efcc622c4242f195f707a08b38606dd3d16`.
- Batas: bukti sel/protokol dan raster font lokal; GUI/font fisik, input client, SSH dan CI belum dibuktikan. Tidak menjalankan security test atau suite seluruh target.

### Hasil koreksi frame terpisah dan celah tipis

- Klarifikasi pengguna: yang diinginkan adalah dua frame dengan celah tipis menyerupai pemisah kiri–kanan, bukan satu border bersama. Implementasi shared border sebelumnya salah menafsirkan tampilan yang dimaksud; keputusan itu digantikan oleh bagian ini. Indikator glyph berikut kemudian dihapus setelah laporan glitch; lihat [hasil penghapusan indikator horizontal](#hasil-penghapusan-indikator-horizontal).
- Kedua frame dikembalikan utuh pada dua baris bertetangga, tanpa baris spacer. Minimum/layout/preflight kembali menjumlahkan tinggi kedua subtree; pane tidak overlap. Hit resize tetap pada border bawah subtree atas, dengan offset drag/keyboard yang sama.
- Unicode/Nerd memakai combining diaeresis below U+0324 pada stroke, corner atau space yang sudah ada. Dua titik berada rendah menuju celah tanpa menghapus garis/sudut; ASCII memakai underscore. Ini tidak menambah row, dependency proyek, atau alokasi string per frame. Junction, budget judul shared border dan renderer judul terpisah yang obsolete dihapus; judul kembali ke frame normal.
- `cargo fmt --all`, **19 tes terarah geometri/UI** dan `cargo build --locked --release --bin naf` lolos tanpa warning build. Matriks 78 frame tetap mencakup empat topologi, tiga profil ikon, lebar 12/25/60 yang muat dan semua fokus; rasio kolom juga memaksa grip menumpang corner untuk memastikan bentuknya tetap utuh.
- Dua sesi release terisolasi: Nerd/truecolor membuktikan glyph di stroke/space/corner, empat panel, mouse/keyboard resize, hit teks/save yang diperiksa isi filenya, collapse 12×6 serta restore rasio. ASCII/none membuktikan dua frame minimum pada 12×11, fallback underscore, grow dan drag. Keduanya exit 0, mengembalikan termios dan menonaktifkan alternate screen; fixture, server tmux dan socket stale dibersihkan.
- Raster dari sel TUI aktual dengan DejaVu Sans Mono lokal pada sel 10×20 memperlihatkan celah kiri–kanan dan atas–bawah yang sebanding. Warna raster disederhanakan untuk inspeksi posisi; bukan screenshot client pengguna. Metrik font/line height tetap dapat mengubah posisi pixel dan bentuk combining glyph; ASCII tersedia bila rendering glyph tidak sesuai.
- SHA-256 release akhir: `316e1c0afbfbd0b78b38e52dd72aabcd70689293f1f873558d1e1b05d18cc60a`.
- Batas: bukti sel/protokol dan raster font lokal; GUI/font fisik, input client, SSH dan CI belum dibuktikan. Tidak menjalankan security test atau suite seluruh target.

### Hasil penghapusan indikator horizontal

- Pengguna melaporkan glitch pada client dan menyetujui penghapusan titik horizontal. Seluruh indikator Rows dihapus, termasuk combining U+0324 Unicode/Nerd dan underscore ASCII; tidak diganti dengan glyph lain.
- Dua frame, jarak, minimum dan hit resize tidak berubah. Renderer hanya menghias gap Columns; hit Rows tetap pada border bawah subtree atas. Helper/urutan overpaint khusus indikator Rows dihapus.
- `cargo fmt --all`, **satu tes regresi UI** dan `cargo build --locked --release --bin naf` lolos tanpa warning. Matriks 78 frame memeriksa seluruh baris border bawah terhadap frame/corner/space normal, mencakup empat topologi, tiga profil ikon, lebar 12/25/60 yang muat dan semua fokus.
- Sesi release Nerd/truecolor pada 120×32: empat panel; drag horizontal tanpa indikator, shortcut Alt+Shift+Up, drag kolom, corner normal tepat di bekas lokasi indikator, serta collapse 12×6 dan restore dengan geometry utuh.
- Sesi release ASCII/none: dua panel minimum 12×11 memiliki border `+----------+` tanpa underscore. Setelah grow ke 80×24, drag/shortcut Rows serta grip `:` dan drag Columns tetap berfungsi.
- Kedua sesi terisolasi exit 0, mengembalikan termios dan menonaktifkan alternate screen; isi fixture tidak berubah. Fixture, server tmux, socket dan helper sementara dibersihkan.
- Batas: bukti sel/protokol TUI aktual, bukan GUI/font fisik client pengguna. Tidak menjalankan security test, suite seluruh target, SSH, packaging atau CI.

### Hasil P1 aksesibilitas dan pencarian

**P1 terpilih selesai; tahap 4 ditutup untuk matriks Linux yang sudah dibuktikan**, sesuai keputusan pengguna setelah batas fisik/non-loopback dijelaskan. Ini bukan persetujuan seluruh usulan P1/P2 audit atau klaim dukungan terminal/platform universal.

- Dua worker implementasi AGY menangani renderer/helper/shortcut dan enumerasi match. Integrator memiliki state/action serta caller `app`, lalu mengintegrasikan dua review read-only AGY.
- Temuan review berupa cache reload dengan revisi yang dipakai ulang, Find maju yang melewatkan overlap, dan hit mouse stale ketika membuka Search diperbaiki dengan regresi. Navigasi konfirmasi default Cancel, konteks path, resize explorer, helper 80 kolom, overflow disabled, prefill dan penghitung/wrap lulus alur nyata.
- Seluruh eksekusi live pada batch ini dikerjakan `atmo/alicn/deepseek-v4.1-flash-1017`; integrator memeriksa laporan, log, data hasil dan screenshot emulator. Audit/security serta clipboard eksternal historis tidak dijalankan ulang; bukti sebelumnya tetap memiliki cakupan dan batasnya di [SECURITY](SECURITY.md#hasil-verifikasi-model-terpisah) dan [clipboard sistem](#hasil-clipboard-sistem-dan-konfigurasi-persisten).

| Pemeriksaan | Hasil dan cakupan |
|---|---|
| Baseline P1 `efa4926d…` | Format check, `cargo test --locked --all-targets` **310/310** (dua example tanpa test), dan build release lolos. Matriks aplikasi **384 cek: 382 lolos, 2 gagal** pada override warna; bukan klaim 384 lolos |
| Fix output warna `49c83ac2…` | Format check, satu regresi backend nyata dengan **5 subprocess terisolasi**, satu regresi helper, dan build release lolos; **21/21 pemeriksaan warna/font terarah** lolos. Suite 310, matriks 384 dan soak tidak diulang atau dialihkan ke hash ini |
| Emulator/font aktual | xfce4-terminal 1.1.4/VTE pada Xvfb: Dark/truecolor/Nerd, Light/256/Unicode, Terminal/16/ASCII dan none/ASCII, termasuk dirty/seleksi/dialog/tiny/split. DejaVu Sans Mono, fallback CJK WQY, serta **JetBrainsMono Nerd Font Mono** upstream v3.5.1 dibuktikan dengan `fc-match` ke file scratch dan screenshot aktual |
| Client tmux 3.5a | Client benar-benar attach melalui PTY; F1/F10, Ctrl+S/Q dan Alt+Shift+Right mencapai aplikasi. Prefix bawaan mengonsumsi satu Ctrl+B; Ctrl+B dua kali meneruskan toggle explorer |
| SSH nyata | OpenSSH ke sshd loopback terisolasi `127.0.0.1:22222`; `SSH_CONNECTION`, edit/save bytes tepat, mouse SGR, resize dan termios remote terbukti. Bukan pengujian host non-loopback |
| Penggunaan panjang | Pada baseline `efa4926d…`: **943,1 detik, 197 siklus, 0 error**; fixture 103.094 byte tetap ber-SHA `28cb058b59165b31e23429a25530c9d11dec28bbd212d49e871cab86e4e2e86e`. Exit 0 dan termios/alternate screen/mouse/bracketed paste dipulihkan |

Sampel proses selama soak baseline; bukan batas CPU/RSS, bukti bebas kebocoran, atau SLA:

| Sampel | CPU kumulatif (s) | RSS (KiB) | VmHWM (KiB) |
|---|---:|---:|---:|
| Awal | 0,01 | 5.624 | 5.684 |
| Tengah | 5,93 | 5.892 | 5.892 |
| Akhir | 11,87 | 5.892 | 5.892 |

Temuan runtime warna diselesaikan, bukan disembunyikan dengan mengubah dokumentasi: `Theme` tetap meresolusikan Auto/`NO_COLOR`/override/None; `terminal::use_theme_color_policy()` mengizinkan backend Crossterm mengeluarkan token tema tanpa menerapkan `NO_COLOR` kedua kali. CLI truecolor/256 dan config truecolor kini menang; Auto+`NO_COLOR` serta None tetap tanpa warna. Tidak ada mutasi environment pengguna atau perubahan raw mode/probe/cleanup.

Regresi baru memakai token accent, sebab foreground/canvas ANSI16 memang `Reset` sesuai desain. Perbaikan kompilasi tes UI dan assertion helper duplikat hanya menyentuh `cfg(test)`; tes helper menilai akses/hit geometry, bukan memaksa dua label F1.

- SHA-256 release baseline P1: `efa4926d7f8c654839f6a6e70314d4157a770d86bfbd0f6cfeec490b5712fdfe`.
- SHA-256 release akhir: `49c83ac2e2278c1441a2b6842e0e2520fe5c55d0d11a123ddfbe495cf5643c10`.
- Cakupan penutupan: Linux x86_64 UTF-8, PTY, VTE/font aktual pada display virtual dengan input XTEST, client tmux dan SSH loopback di atas. Layar/input/font fisik pengguna, host SSH non-loopback, OS/emulator lain, palette/line-height kustom dan compositor glass tidak diklaim teruji. Discovery host tambahan tidak memberi akses autentikasi; tidak menebak credential.
- Batas operasional dan hasil ukuran/baris besar tetap mengikuti [PRODUCT](PRODUCT.md#usulan-batas-dukungan-awal), [profil dokumen besar](#hasil-status-panjang-dan-profil-dokumen-besar) serta [geometri sparse](#hasil-geometri-sparse-untuk-baris-panjang); penutupan ini tidak mengubah batas menjadi jaminan latensi atau memori.
- Publish, installer, CI remote dan packaging ulang bukan bagian batch ini. Penutupan penggunaan harian tidak menyatakan distribusi publik siap atau menghapus batas keamanan/filesystem yang sudah dicatat.
- Cleanup: seluruh worker run telah exit dan terminalnya dilepas; tmux/Xvfb/sshd, socket, fixture, key, font/paket scratch serta driver uji dibersihkan. Evidence/log/laporan dipertahankan terpisah dari fixture; sumber ringkasan adalah `/tmp/naf-tahap4-verifikasi-final.md` dan `/tmp/naf-final-verify/evidence/`. Tidak menyentuh buffer/config/snapshot pengguna.

### Hasil indent dan dedent blok

- Pengguna menyetujui fitur editing berikutnya: indent/dedent blok, bukan scrollbar atau persiapan publish. Tab pada seleksi multibaris menambah empat spasi per baris; Shift+Tab menghapus satu tab awal atau hingga empat spasi. Range setengah-terbuka, blank/EOF, arah seleksi dan normal Tab satu baris mengikuti [kontrak](ARCHITECTURE.md#kontrak-indentdedent-blok).
- `Document` menyimpan batch prefix ringkas sebagai satu history checkpoint, terpisah dari splice biasa; pemetaan view meminjam metadata history, tidak menyalin seluruh blok atau membuat String per baris. Forward diterapkan dari belakang, inversenya dari depan. Seluruh 17 caller bool workspace dimigrasikan ke `EditViewPolicy`; tidak ada shim.
- Seluruh tes/live dilakukan `atmo/alicn/deepseek-v4.1-flash-1017`. Integrator membaca log, laporan dan screen aktual tanpa menjalankan ulang tes. Review read-only dialihkan ke model itu dengan persetujuan pengguna setelah AGY tidak tersedia; tidak ada klaim review AGY lolos pada batch ini.
- Review read-only selesai tanpa temuan defect source-level pada empat kombinasi indent/dedent × forward/inverse, peminjaman metadata history, checkpoint dirty, remap shared view dan isolasi routing. Guard marker-vs-history tetap merupakan invariant internal yang dijaga semua jalur mutasi; tidak ditambah fallback palsu. Pemetaan inverse dedent prefix campuran/CRLF pada view lain ditelusuri manual, bukan diklaim memiliki regresi khusus.

| Pemeriksaan terarah | Hasil |
|---|---:|
| `cargo test --locked --bin naf document::tests` | 25 lolos |
| `cargo test --locked --bin naf workspace::tests` | 11 lolos |
| `cargo test --locked --bin naf app::tests` | 77 lolos |
| Skenario live tmux 3.5a/PTY | 23 lolos |

- `cargo fmt --all -- --check`, build release `--locked --release --bin naf` serta CLI/F1 help berhasil. Lima regresi `indent_lines_*` dan empat `block_*` ikut berjalan; suite seluruh target tidak dijalankan.
- Live membuktikan seleksi maju/balik dan endpoint awal baris berikutnya, Tab berulang, blank/EOF tanpa newline tambahan, tab stop CJK, dedent tab/1–4 spasi/prefix campuran/combining/ZWJ, no-op yang mempertahankan redo, serta satu undo yang memulihkan 200 baris sekaligus. Save BOM/CRLF mempertahankan bytes format; read-only dan modal/field/explorer tidak menerima edit bocor.
- Dua pane shared dengan caret/seleksi interior pada baris berbeda menjaga posisi logis dan viewport setelah prefix berubah; undo/redo lintas pane mempertahankan satu teks/history. Resize 120×36→90×28→140×40 serta klik SGR pada teks bergeser sesuai kolom render. Find sesudah mutasi memperbarui match/posisi, bukan memakai hasil stale.
- Sesi exit 0; termios sebelum/sesudah identik dan alternate screen/mouse/bracketed paste dipulihkan. Input berasal dari protokol tmux/PTY dan CSI Z untuk Shift+Tab, bukan perangkat fisik atau GUI/font baru. CSI-u/Kitty tidak dinegosiasikan dan tidak menjadi fallback; event Tab+Shift juga dicakup unit aplikasi.
- SHA-256 release teruji: `fb0afd0d3268a654beb859fa37355b928e3dc8527787800a57416f1d7510ab6d`.
- Batas: tidak mengulang audit keamanan, soak 15 menit, seluruh matriks font/SSH tahap 4, CI atau packaging. Ini bukan SLA performa/memori dan tidak mengubah batas operasional file/history.
- Driver, HOME/XDG, server/socket dan fixture uji dibersihkan setelah hasil diperiksa; laporan serta log/screen/termios dipertahankan di `/tmp/naf-indent-live-final.md` dan `/tmp/naf-indent-evidence/`. Tidak mengubah file, snapshot, konfigurasi atau font pengguna.

### Hasil scrollbar editor

- Pengguna menyetujui scrollbar vertikal/horizontal per view editor, bukan scrollbar explorer/help/field atau persiapan publish. Track di dalam frame, klik page, drag thumb, ukuran minimum dan lebar horizontal mengikuti [kontrak](ARCHITECTURE.md#scrollbar-editor).
- Integrator memformat source yang berubah dan membangun release; seluruh tes dan live dijalankan `atmo/alicn/deepseek-v4.1-flash-1017`. Dua tugas QA independen memakai fixture/HOME/XDG serta tmux/PTY terisolasi, tanpa edit repository. AGY tidak dipakai pada batch ini.
- Putaran awal menemukan dua kegagalan tes serta tiga defect runtime: rail satu sel dengan thumb nol pada gutter tujuh digit; offset horizontal pane nonaktif tidak dinormalisasi saat lebar bertambah; kembali ke sel endpoint thumb terkuantisasi mempertahankan offset lama. Integrator memperbaiki ketiganya, memisahkan redraw metadata dari invalidasi syntax, menghapus assertion timing internal satu-render-step, serta memigrasikan fixture cursor ke ukuran yang masih memuat barisnya.

| Filter tes `cargo test --locked --bin naf` | Putaran awal | Setelah perbaikan |
|---|---|---|
| `document::tests` | 25 lolos | Tidak diulang; source tidak berubah |
| `workspace::tests` | 11 lolos | Tidak diulang; source tidak berubah |
| `app::tests` | 78 lolos, 1 gagal | 80 lolos, termasuk regresi resize pane nonaktif |
| `ui::tests` | 39 lolos, 1 gagal | Regresi cursor yang gagal: 1 lolos; 39 lainnya tidak diulang |
| `chrome::tests` | 4 lolos | 4 lolos, termasuk minimum track dua sel |
| `scrollbar::tests` | 3 lolos | 3 lolos, termasuk endpoint terkuantisasi |

- Recheck akhir berjumlah **88 tes terarah lolos**, tanpa suite seluruh target. Build `cargo build --locked --release --bin naf` berhasil. Integrator membaca log tes dan tangkapan layar aktual, tidak menjalankan ulang tes.
- Live awal membuktikan track/page/drag kedua sumbu, view shared F8/F9, wheel yang berbatas sama, cursor/seleksi/dirty tetap, CJK/tab/combining/ZWJ/CRLF, refresh sesudah edit/undo, resize kecil dan fallback, ASCII/Unicode/Nerd serta output background default terminal. Pada build awal `d9f03e5…`, pengukuran lebar baris sekitar 4 MiB selesai sekitar 9,27 detik setelah launch; input tetap diproses ketika `?` tampil. Ini observasi satu host, bukan SLA atau jaminan giant grapheme.
- Live build akhir mengulang kasus yang terdampak: pada 12×18 dengan satu juta baris, horizontal rail disembunyikan, row tambahan menjadi teks dan vertical/keyboard/wheel tetap berfungsi; smoke 80×24 serta 100×35 menjaga kedua sumbu. Resize divider dan terminal dengan tinggi tetap menormalisasi pane nonaktif sampai satu sel caret EOL, bukan halaman kosong. Press/no-motion tidak melompat; kembali ke sel pertama/terakhir mencapai 0/maksimum tanpa keluar track.
- Resize pane aktif tetap boleh mengikuti caret seperti sebelumnya; normalisasi viewport tidak mengubah cursor/seleksi/fokus. Release lintas pane, ketik/undo sesudahnya dan pemulihan setelah resize lulus. Exit 0 dan termios identik; alternate screen, mouse dan bracketed paste dipulihkan pada pemeriksaan yang direkam.
- SHA-256 release akhir teruji: `16677d3446c62b94aaf107512d8b14c4658dbb89304caeffa069c9056a58a357`.
- Batas: input/protokol dan output berasal dari tmux/PTY Linux, bukan mouse/font/glass fisik. Tidak mengulang audit keamanan, soak, font/SSH matrix tahap 4, CI atau packaging; pengukuran 4 MiB tidak diulang setelah perbaikan batas. Penutupan tahap 4 sebelumnya tidak diperluas ke platform baru.
- Laporan awal: `/tmp/naf-scrollbar-core-final.md` dan `/tmp/naf-scrollbar-edge-final.md`; recheck: `/tmp/naf-scrollbar-recheck-core-final.md` dan `/tmp/naf-scrollbar-recheck-edge-final.md`. Log/screen/termios dipertahankan pada direktori `*-evidence` masing-masing. Fixture, HOME/XDG, driver dan prompt sementara dibersihkan; seluruh worker sukses telah exit dan terminalnya dilepas. Rekam startup gagal tetap historis dengan `ownership_transferred`, bukan proses aktif yang perlu ditutup paksa.

### Hasil scrollbar Explorer dan F1

- Batch lanjutan menambahkan rail vertikal pada tree dan katalog bantuan, dengan [kontrak geometri/input](ARCHITECTURE.md#scrollbar-explorer-dan-f1). Scrollbar horizontal Explorer, indikator overflow field, theme/keymap runtime serta publish tetap terpisah.
- Integrator memformat lima file source yang berubah dan membangun `cargo build --locked --release --bin naf`. Tes dan TUI nyata dijalankan dua tugas read-only `atmo/alicn/deepseek-v4.1-flash-1017` pada binary beku yang sama, memakai fixture/HOME/XDG dan server tmux/PTY sendiri. AGY tidak dipakai.

| Filter `cargo test --locked --bin naf` | Kasus berbeda yang lulus |
|---|---|
| `app::tests` | 82 |
| `ui::tests` | 40 |
| `chrome::tests` | 4 |
| `help::tests -- --test-threads=1` | 4 |

- Total **130 kasus berbeda lulus, 0 gagal**, termasuk regresi `explorer_scrollbar_preserves_selection_focus_and_document_and_rejects_changed_rows` dan `help_scrollbar_preserves_suspended_field_and_cancels_on_resize_or_keys`. Filter chrome tercatat berulang pada log worker; angka tabel menghitung kasus berbeda, bukan jumlah eksekusi. Suite seluruh target tidak dijalankan.
- Live Explorer membuktikan klik page, drag/no-motion/endpoint, wheel clamp, release lintas editor, pilihan/fokus/caret/dirty/undo tetap, expand/collapse/refresh, parent row serta divider independen. Folder kosong/satu entry memiliki rail disabled; tinggi minimum 120×6 menyembunyikan rail dan tetap menerima keyboard/wheel. Hide, resize sempit dan perubahan konteks membatalkan capture lama; resize tetap boleh memunculkan pilihan sesuai perilaku sebelumnya.
- Live F1 membuktikan rail/footer terpisah, page/drag/wheel/keyboard sampai akhir katalog, wrapping dan limit yang konsisten pada 80×24, 120×36, 40×10 dan 20×6; 19×5 memakai tiny tanpa rail. Resize saat End atau thumb ditahan tidak menyisakan drag. Find/Replace, Open/Save As serta konfirmasi unsaved kembali dengan state yang sama; pointer di belakang popup tidak diteruskan ke editor/tree.
- Smoke editor kedua sumbu tetap menjaga cursor/seleksi; key membatalkan capture. ASCII/Unicode/Nerd opt-in, SGR default terminal, CLI/F1 katalog gesture serta exit 0 dan pemulihan termios/alternate-screen/mouse/paste dibuktikan dari capture/stream. Integrator membaca hasil tes dan tangkapan layar aktual, tidak menjalankan ulang tes.
- SHA-256 release teruji: `90de52fe93c40cbcfc48fb46050f9d2eba1f1624ab07e75bf143da04eb318d32`.
- Batas: Linux tmux/PTY dengan input mouse SGR sintetis, bukan mouse/font/glass fisik. Burst event live tidak menjamin batching sebelum redraw; guard stale-hit juga dicakup regresi unit. Help selalu lebih panjang dari viewport, sehingga rail disabled pada konten yang muat bukan skenario live Help. Tidak mengulang audit keamanan, soak, SSH/font matrix, 4 MiB, CI atau packaging; tidak memperluas penutupan tahap 4 ke platform baru.
- Laporan: `/tmp/naf-nav-scroll-core-final.md` dan `/tmp/naf-nav-scroll-help-final.md`; log, capture dan stream berada di `/tmp/naf-nav-scroll-core-evidence/` dan `/tmp/naf-nav-scroll-help-evidence/`.
- Fixture/HOME/XDG, driver, helper dan prompt sementara milik batch dibersihkan setelah hasil diperiksa; laporan serta bukti runtime dipertahankan. Server tmux uji sudah dihentikan oleh worker dan kedua terminal worker sukses telah dilepas. Startup Help yang gagal memakai terminal yang kemudian berhasil dipakai retry; terminal tersebut tidak ditutup paksa melalui rekam gagal.

### Hasil indikator overflow field

- Lanjutan UX menambahkan indikator pasif pada field path, Find dan Replace sesuai [kontrak field](ARCHITECTURE.md#keputusan-pemolesan-field-dan-bantuan). Perubahan produksi hanya pada `field_ui`; model/riwayat field, controller input, clipboard, I/O dan dependency tidak diubah.
- Integrator memformat `src/field_ui.rs` dan berhasil membangun `cargo build --locked --release --bin naf`. Tes dan smoke awal dikerjakan `lab/atmo/cx/gpt-6.1-terra`; penutupan bukti TUI memakai `openai-codex/gpt-6.1-terra` sesuai pilihan pengguna. Tidak menjalankan security test/audit, suite luas, matriks font/SSH atau benchmark.
- Putaran awal: filter `field_ui::tests::overflow_` lulus dua regresi, mencakup indikator tiga profil, pemetaan hit/caret dan minimum satu–tiga sel dengan grapheme lebar/combining. Filter `field_ui::tests::test_horizontal_window_caret_following` gagal karena assertion Home lama masih mengharapkan kolom 0, bukan awal teks setelah gutter.
- Integrator memigrasikan assertion Home ke awal area teks/byte 0 dan End ke stop caret/akhir teks, bukan mengunci koordinat layar baru. Ini perubahan tes saja; produksi dan binary tidak diubah. Dua regresi yang sudah lulus tidak dijalankan ulang.
- Recheck hanya filter caret tersebut lulus satu kasus. Gabungan hasil mencakup **tiga kasus berbeda lulus**, bukan pengulangan suite: dua regresi overflow dari putaran awal dan satu kontrak caret setelah migrasi.
- Smoke awal membuktikan arah indikator Open path, field panjang/pendek, kembalinya F1 dan exit 0. Penempatan byte klik, clipboard penuh serta hit setelah resize belum dibuktikan oleh smoke awal; posisi cursor sesudah redraw bukan bukti tunggal karena window mengikuti caret.
- Recheck otomasi ekspor belum menghasilkan output: capture akhir masih menampilkan dialog Replace dan path tujuan berada di field Replacement. Langkah Save As belum tercapai, sehingga ini belum membuktikan kegagalan I/O aplikasi. Driver bukti final memisahkan bare Esc dari tombol selanjutnya dan memeriksa state dialog sebelum ekspor; tidak mengubah kode aplikasi.
- Driver final dijalankan sekali dan seluruh enam check lulus: seleksi parsial path bertahan saat klik gutter, gutter field nonaktif tidak memindahkan fokus query, F1 menjaga field/fokus, klik token pada 80×24 → 40×10 → 80×24 memilih byte yang benar, clipboard query/replacement penuh dan fixture asli tidak berubah.
- Hasil Save As `selected.txt.actual` identik dengan expected `TAIL5`; `query.txt.actual` identik dengan query penuh beserta tiga penanda edit pada token yang diklik, tanpa prefix/indikator tambahan. Driver dan aplikasi keluar 0. Integrator membaca log tes, JSON hasil, kedua pasangan output penuh serta capture resize dan dialog Save As; tidak menjalankan ulang tes.
- SHA-256 binary release: `02aac5035f60d3cbbcec572920715073890788ffe959cac5e34f3f30d31bb73c`.
- Laporan awal `/tmp/naf-field-overflow-terra-final.md`, dengan log dan capture di `/tmp/naf-field-overflow-terra-evidence/`.
- Laporan recheck `/tmp/naf-field-overflow-terra-recheck-final.md` dan capture di `/tmp/naf-field-overflow-terra-recheck-evidence/` menyimpan batas tersebut; log tes caret berada di `/tmp/naf-field-overflow-terra-recheck-test.log`.
- Bukti penutupan: `/tmp/naf-field-overflow-terra-proof-final.md`, `/tmp/naf-field-overflow-terra-proof-evidence/` dan `/tmp/naf-field-overflow-terra-proof.log`. Pasangan expected/actual dipertahankan di evidence sesudah fixture dihapus.
- Batas: TUI nyata Linux melalui tmux/PTY dengan mouse SGR sintetis, bukan perangkat mouse/font/glass fisik. Unicode/Nerd/ASCII serta ukuran nilai satu–tiga sel tercakup pada tes renderer; smoke akhir tidak mengulang matriks profil atau clipboard sistem. Penutupan tahap 4 tidak diperluas ke platform baru.
- Seluruh fixture/HOME/XDG, driver dan prompt sementara milik batch dibersihkan; laporan, log dan capture dipertahankan. Server tmux uji dihentikan, terminal worker selesai dilepas dan assignment jalur CX dihentikan terkonfirmasi saat provider dialihkan. Tidak menutup terminal atau mengubah konfigurasi akun pengguna.

### Hasil persistensi Appearance

- Laporan pengguna bahwa F11 dipakai window manager dan style hilang setelah restart diterima sebagai repro; tidak mengulang pengujian window manager untuk konfirmasi. F11 tidak lagi binding aplikasi; Alt+P serta chip Style menjadi pembuka Appearance.
- Apply menyimpan tiga preferensi visual melalui `config::save_merged` dengan Source/Env peluncuran sebelum mengubah tampilan. Clipboard tersimpan tidak ikut diganti oleh override runtime. Save gagal menyisakan draft/fokus/error, tampilan lama dan kesempatan retry; `--no-config` sengaja run-only. [Kontrak aktif](ARCHITECTURE.md#kontrak-switcher-tampilan-runtime).
- Regresi, build dan live test dikerjakan `atmo/alicn/deepseek-v4.1-flash-1017`; tidak menjalankan security/live test melalui GPT atau AGY. Tidak ada fix produksi tambahan selama QA.

| Pemeriksaan | Hasil |
| --- | --- |
| `cargo fmt --all` dan `cargo fmt --all -- --check` | Exit 0 setelah edit berhenti |
| `cargo test --locked --bin naf appearance_` | 8 kasus unik lulus, 0 gagal, 348 filtered: 4 lama dan 4 regresi baru |
| `cargo build --locked --release --bin naf` | Exit 0; binary release baru dibangun |
| Default XDG dan restart | F11 PTY tidak membuka modal; Alt+P/Apply menyimpan light/256/nerd. Proses kedua tanpa override visual memulihkannya; Cancel menjaga hash file |
| `--config PATH` | Apply menulis file kustom terminal/16/ascii, bukan lokasi default |
| `--no-config` | Tampilan runtime berubah; hint/status run-only dan hash file tetap |
| Save gagal pada 40×10 | TOML dirusak setelah startup; Apply mempertahankan modal/draft dengan error terlihat, file invalid tetap |

Delapan tes sempat dieksekusi ulang lewat binary test langsung karena worker menganggap stdout command hilang, meskipun transcript/log lengkap tersedia; tetap delapan kasus unik, bukan 16. Anomali visibilitas output dilaporkan, tanpa pengulangan tes oleh integrator. Live test mencakup lima skenario perilaku dan enam invokasi: satu startup `--config` ditolak karena fixture belum ada, lalu fixture disiapkan; lima invokasi yang berjalan keluar 0. Tidak mengubah produksi demi fixture/selector driver.

SHA-256 binary live `target/release/naf`: `2833623049305c78a4bf9a0e39bb0b4d5048965f321475dbf32d9394cdba27d2`. Laporan `/tmp/notefart-appearance-persist-qa-report.md` dan capture/log `/tmp/notefart-appearance-persist-evidence/` merekam command, exit, tampilan serta hash aktual. HOME/XDG/CWD dan server tmux terisolasi; fixture/driver dibersihkan, evidence dipertahankan.

Batas: bukan pengujian window manager fisik, font/Nerd Font, glass, clipboard eksternal, suite seluruh target atau CI hosted. Binary terpasang akun pengguna belum diganti; jalankan ulang packaging dari binary release baru dan installer `--replace` sebagai akun tujuan. Catatan switcher F11/run-only di bagian berikut adalah bukti kontrak historis, bukan perilaku terbaru.

### Hasil switcher tampilan runtime

- Appearance menyediakan draft tema, mode warna dan profil ikon melalui F11/Alt+P, Apply/Cancel, keyboard/mouse dan return F1 sesuai [kontrak switcher](ARCHITECTURE.md#kontrak-switcher-tampilan-runtime). F12 tetap Close pane; menu tidak ditumpuk di atas modal lain.
- Preferensi hanya berlaku untuk run ini: tidak menyimpan konfigurasi/sesi atau mengganti clipboard/font. `Options` menyimpan pilihan yang diminta, termasuk Auto; palet hanya dibangun ulang jika tema/warna berubah dan token syntax tetap semantik.
- Dua worker implementasi serta QA memakai `openai-codex/gpt-6.1-terra`. Review fungsional read-only tidak menemukan bug consumer-visible pada bagian yang diperiksa; bukan audit keamanan, suite seluruh target atau perluasan matriks platform.
- Integrator memformat `src/app.rs`, `src/ui.rs`, `src/shortcuts.rs`, `src/theme.rs` dan berhasil menjalankan `cargo build --locked --release --bin naf`.
- Satu kali `cargo test --locked --bin naf appearance_` lulus **empat kasus**, 0 gagal, 330 filtered. Dua regresi controller mencakup draft/F1/Cancel/Apply, clipboard/dokumen/viewport tetap, siklus mouse, klik stale/tiny serta Apply saat fokus Cancel. Dua regresi renderer mencakup geometri/fokus minimum tiga profil dan penghapusan target lama pada tiny.
- Smoke awal berhenti karena assertion driver mencari kata `Appearance` di seluruh layar, termasuk status sukses. Capture menunjukkan menu sudah tertutup setelah klik Apply dari fokus Cancel, profil ASCII/Auto mono aktif di bawah `NO_COLOR`, viewport tetap baris 25 dan caret Ln 1. Ini kegagalan selector driver, bukan bukti bug aplikasi; empat tes yang sudah lulus tidak diulang.
- Driver lanjutan mendeteksi row menu, bukan kata pada status. Satu eksekusi oleh Terra Codex menyelesaikan sisa smoke tanpa mengubah produksi: Apply keyboard menjaga baris atas 25/caret Ln 1, reopen tetap menunjukkan requested Auto, Auto mono di bawah `NO_COLOR` dan explicit Truecolor mengembalikan escape RGB; Nerd menampilkan glyph private-use.
- TUI 40×10 menampilkan seluruh row/aksi, Tab/BackTab memindahkan fokus; pada 39×9 input Apply/cycle tersembunyi diabaikan, F1 kembali ke draft dan Esc membatalkan. Setelah resize, nilai draft serta live diperiksa kembali; resize tetap boleh mengikuti kebijakan viewport yang sudah ada.
- Edit menambah karakter dan dirty marker, undo mengembalikan teks/status bersih. Driver serta aplikasi exit 0; hash fixture, konfigurasi dan binary identik sebelum/sesudah. Integrator membaca log/exit empat tes, hasil JSON, hash serta capture F1, Apply, minimum/tiny, reopen dan undo.
- SHA-256 binary release: `44536f2d013daf0079944dd201c24b423ed95a3ee35c3f6a38bdff762c169485`.
- Laporan dan bukti awal: `/tmp/naf-appearance-qa-final.md` serta `/tmp/naf-appearance-qa-evidence/`, termasuk output/exit tes, capture dan hash fixture/config/binary yang identik sebelum/sesudah.
- Laporan penutupan: `/tmp/naf-appearance-recheck-final.md` dan `/tmp/naf-appearance-recheck-evidence/`, termasuk `results.json`, capture plain/SGR serta exit/hash aktual.
- Batas: Linux melalui tmux/PTY dan mouse SGR sintetis; bukan bukti mouse/font/glass fisik, clipboard sistem, Save As, SSH, performa atau audit keamanan. Penutupan tahap 4 tidak diperluas ke platform baru.
- Fixture/HOME/XDG, driver dan prompt sementara milik batch dibersihkan; laporan, log, capture dan manifest hash dipertahankan. Kedua sesi tmux sudah berhenti dan keempat terminal worker sudah dilepas. Empat rekam startup gagal tetap historis dengan `ownership_transferred`; guarded release tidak melakukan aksi proses atau menutup terminal pengguna.

### Hasil ergonomi field dan navigasi Explorer

- Lanjutan task pengguna mencakup undo/redo delta field, penghapusan kata, drag autoscroll, type-ahead jump dan visibilitas error Explorer sesuai [kontrak](ARCHITECTURE.md#kontrak-ergonomi-field-dan-navigasi-explorer). Filter, hover dan toleransi double-click field tidak ditambahkan.
- Model field dan Explorer dikerjakan `openai-codex/gpt-6.1-terra`; integrator memiliki controller App dan kontrak antarmuka. Dukungan protokol modifyOtherKeys dikerjakan `atmo/alicn/deepseek-v4.1-flash-1017`. Tidak ada dependency atau format sesi baru.
- Integrator memformat tujuh file sumber yang berubah dan berhasil menjalankan `cargo build --locked --release --bin naf`. Hash awal batch: `fd39c86260f6581628c56a03aa457038994bea8849633f38b990ecb7fd77a2de`.
- QA normal Terra menjalankan empat filter sekali: sembilan kasus `field::tests::`, dua `app::tests::polish_app_`, satu `explorer::tests::polish_explorer_` dan satu `app::tests::field_clipboard_isolated_from_document`. **13 kasus lulus**, tanpa menjalankan filter input/negative security.
- Smoke normal membuktikan history query dan return F1, Ctrl+Delete, replacement terpisah serta window yang terus bergeser selama pointer diam di luar tepi; release menghentikannya. Driver kemudian mengirim literal tmux `C-Shift-s` alih-alih F4, sehingga guard Save As gagal sebelum path tujuan dimasukkan. Ini kegagalan otomasi; ekspor byte, Explorer runtime, resize dan exit normal belum dibuktikan oleh smoke awal. Tes yang sudah lulus tidak diulang untuk konfirmasi.
- DeepSeek menjalankan tiga filter target: delapan kasus `input::tests::polish_keys_`, satu `workspace::tests::polish_explorer_` dan satu `ui::tests::polish_explorer_`, semuanya lulus. Bersama 13 kasus normal, ini **23 kasus target berbeda** pada hash awal; bukan suite seluruh target.
- Kesalahan loop worker DeepSeek sempat menjalankan filter lebih lebar `input`, `workspace` dan `ui` sebelum command exact diperbaiki. Laporan mencatat hasil tersebut terpisah; angka target di atas bukan jumlah seluruh eksekusi. Semua pengujian input/negative dan error permission tetap dikerjakan DeepSeek via atmo, bukan GPT/AGY.
- Smoke DeepSeek membuktikan Ctrl+Backspace/Shift+Z melalui modifyOtherKeys serta Ctrl+Delete legacy pada field nyata; dokumen belakang tetap. Kartu root permission mempertahankan penyebab, F2 dapat fokus, Up absolut dan F5 setelah penyebab diperbaiki memulihkan listing, serta resize narrow tetap menyediakan exit. Empat proses keluar 0; perubahan mode/direktori fixture disengaja, sedangkan binary/config/dokumen identik.
- Review scoped menemukan satu affordance keliru: cwd dihapus dan root `.` memiliki parent kosong, tetapi UI menawarkan Up dan menyatakan sudah di filesystem root. Integrator menonaktifkan parent row/Up pada kartu, status dan helper bila parent tidak diketahui; Open helper juga tidak diterbitkan tanpa listing. Backspace manual menghasilkan warning, Retry tetap tersedia. Tidak menebak parent atau mengubah guard filesystem.
- Recheck DeepSeek pada hash akhir lulus satu regresi baru `app::tests::polish_unavailable_parent_is_not_advertised_or_misreported_as_root`, dengan tiga profil ikon internal; gabungan mencakup **24 kasus target berbeda**. TUI deleted-cwd menunjukkan penyebab/Retry tetap tampak, tanpa parent row/Up/Open, Backspace menjadi warning dan F5 tetap mencoba kembali; dokumen/root tidak berubah dan aplikasi exit 0.
- Smoke normal lanjutan memakai driver dengan F4 dan selesai pada hash akhir: ekspor query 1.349 byte identik dengan expected, type-ahead b/b/br, timeout dan reset navigasi terlihat pada marker row, resize 40×10 → 80×24 menjaga undo/F1 dan aplikasi exit 0. Query/drag di sesi baru merupakan prasyarat ekspor yang sebelumnya belum tercapai, bukan pengulangan 13 tes yang sudah lulus.
- Hash akhir build release: `3cfac7ff3831c03a8f1ff28aa911a10ffb08a548209b76afaabb1ae47e8ca676`. Binary/config/tiga fixture input smoke normal tidak berubah; hanya output baru dibuat. Snapshot font/glass/mouse fisik, clipboard sistem, SSH dan kesiapan publish tidak diklaim.
- Bukti awal `/tmp/naf-polish-qa-final.md` dan `/tmp/naf-polish-qa-evidence/`; laporan implementasi `/tmp/naf-polish-field-final.md`, `/tmp/naf-polish-explorer-final.md` serta `/tmp/naf-polish-keys-final.md`.
- Bukti DeepSeek awal `/tmp/naf-polish-keys-qa-final.md` dan `/tmp/naf-polish-keys-qa-evidence/`; bukti penutupan normal `/tmp/naf-polish-recheck-final.md` dan `/tmp/naf-polish-recheck-evidence/`, termasuk output expected/actual, hasil JSON, capture plain/SGR dan hash.
- Bukti recheck finding `/tmp/naf-polish-parent-recheck-final.md` dan `/tmp/naf-polish-parent-recheck-evidence/`. Driver recheck sempat gagal pada assertion parsing frame, lalu diperbaiki dan skenario bersih diulang untuk merekam exit; laporan menyimpan rincian eksekusi, bukan klaim satu percobaan tanpa kegagalan. Regresi unit baru tetap dijalankan sekali, tanpa mengulang input tests.
- Integrator memeriksa output target, hasil JSON, pasangan ekspor, hash dan capture history/drag, prefix, minimum, modified keys, recovery serta affordance akhir. Source/Cargo/binary tidak berubah selama recheck final menurut digest worker; batas encoding terminal dan matriks fisik tetap berlaku.
- Fixture/HOME/XDG, driver dan tujuh prompt sementara milik batch dibersihkan; laporan, log, capture, output expected/actual dan manifest hash dipertahankan. Tujuh terminal worker sudah dilepas; empat rekam startup gagal tetap historis `ownership_transferred` tanpa aksi proses/penutupan terminal pengguna.









## Disiplin verifikasi

- Gunakan unit test untuk invariant dokumen, transaksi, checkpoint, posisi view, dan layout; bukan snapshot wording atau source text.
- Gunakan temporary directory dan fault injection terisolasi untuk I/O; jangan mengubah permission/disk pengguna untuk mengetes kegagalan.
- Buktikan interaksi TUI melalui aplikasi nyata, bukan hanya mock event.
- Tambahkan regression test ketika bug perilaku belum tercakup test yang ada.
- Setelah perubahan terverifikasi, perbarui dokumen terkait dan changelog ketika implementasi mulai tersedia. Hapus spike yang sudah tidak diperlukan.

## Fitur tambahan — session restore

Fitur tambahan ini kini **tersedia untuk exit normal**, bukan perluasan menuju IDE atau syarat baru batch clipboard/konfigurasi yang sudah selesai.

- `naf` tanpa path memulihkan file/buffer, tab dan urutannya, panel aktif, layout split, cursor/selection/scroll dan root/visibility/fokus/lebar explorer. Untitled serta perubahan belum disimpan ikut disimpan, bukan sekadar daftar path.
- Pengguna memilih aktivasi otomatis. Ctrl+Q menyimpan sesi lalu keluar hanya jika sukses; `--no-session` dan launch file/folder tetap terpisah dengan Save/Discard/Cancel. Close tab/panel tetap memeriksa dirty final view.
- Snapshot privat tidak dienkripsi; lokasi/batas/lock/format ditetapkan di ARCHITECTURE.md. Restore memulihkan buffer persis dan baseline asli, bukan reload atau overwrite sumber secara diam-diam. Undo/redo dimulai ulang setelah restore.
- Session restore berbeda dari crash recovery: tidak ada autosave atau snapshot baru saat signal/panic/proses mati. Perubahan/discard dalam sesi yang berakhir abnormal tidak mengganti snapshot exit normal sebelumnya.

## Distribusi — persiapan installer dan draft rilis

Installer bundle GNU/Linux x86_64, helper paket bersama CI dan workflow draft-release manual sudah diimplementasikan. Jalur teruji lulus verifikasi di bawah. Kontrak di [ARCHITECTURE.md](ARCHITECTURE.md#kontrak-distribusi-linux-dan-installer): user-local tanpa sudo otomatis, empat preferensi tanpa menimpa konfigurasi lama, serta publikasi yang tetap menjadi keputusan terpisah. Tidak membuat tag, mengubah lisensi/visibilitas repository atau menjalankan publish.

### Hasil persiapan distribusi

QA biasa memakai `openai-codex/gpt-6.1-terra` melalui Codex; pengujian integritas/negative, perbaikan temuan dan recheck memakai `atmo/alicn/deepseek-v4.1-flash-1017`. Tidak menjalankan ulang suite Rust karena sumber Rust tidak berubah; keberhasilan CI sebelum batch ini berasal dari konfirmasi pengguna.

| Lingkup | Bukti yang diamati |
| --- | --- |
| Packaging dan CLI nyata | Helper dipanggil dari CWD berbeda dengan output berspasi; bundle/checksum valid, `install.sh --help` dan binary hasil instalasi menghasilkan exit 0 / `naf 0.1.0`. |
| Body smoke CI asli | Diekstrak tanpa modifikasi dan dijalankan pada fixture: prefix berspasi, CWD di luar bundle, empat override, roundtrip config lewat aplikasi dan replay config berkomentar byte-identik; exit 0. |
| Default dan tanpa config | HOME sementara + XDG unset memakai `HOME/.local/bin` serta config Unicode di fallback HOME. `--skip-config` bekerja dengan prefix absolut tanpa HOME/XDG. |
| Integritas installer | Binary berbeda ditolak tanpa `--replace`, update eksplisit berfungsi; config lama termasuk sentinel tetap byte-identik. Target final symlink/nonregular ditolak, hardlink pointee tidak berubah, staging gagal tidak menghapus target, config baru 0600 dan mode direktori lama tetap. |
| Penutupan temuan ringan | SemVer malformed ditolak melalui helper sebenarnya, prerelease/build metadata tetap utuh, output path control di-escape. Syntax shell dan parsing YAML lulus setelah edit. |
| Guard tag | Body step draft asli diuji dengan transport GH fixture: SHA cocok mencapai permintaan draft; SHA berbeda/API gagal abort sebelum create. Ini unit logic tanpa API/write/publikasi nyata. |

F2 diperbaiki dengan resolusi namespace `tags/` dan perbandingan commit build sebelum membuat draft; F3/F4 dengan SemVer konsisten dan escaping output. F1 menjadi batas trust yang disengaja: alias direktori parent tetap diikuti, seluruh parent harus dipercaya, dan root pada tree shared/untrusted tidak didukung. Ini bukan containment. Pemeriksaan tag tidak mengunci ref secara atomik terhadap force-update setelah check; draft/ref/asset tetap harus ditinjau sebelum publish.

Binary yang dipakai sudah tersedia sebelumnya, bukan build baru batch ini: SHA-256 `3cfac7ff3831c03a8f1ff28aa911a10ffb08a548209b76afaabb1ae47e8ca676`. Archive recheck akhir SHA-256 `a6a3e06d5f7c10bb14e32d45ee15cafab963cb0135d0b51f8c058a56de916f67`, `VERSION=0.1.0`, checksum cocok. Hash archive mengandung pengaruh mtime staging, bukan janji build reproducible.

Laporan/log: `/tmp/notefart-dist-normal-qa-report.md`, `/tmp/notefart-dist-normal-qa.log`, `/tmp/notefart-dist-security-qa-report.md`, `/tmp/notefart-dist-security-qa-logs/`, `/tmp/notefart-dist-close-findings-report.md` dan `/tmp/notefart-dist-close-findings-logs/`. Fixture HOME/prefix, driver dan prompt sementara dibersihkan; laporan/log dipertahankan.

Batas: hosted CI/workflow rilis dan API GitHub belum dijalankan; parsing YAML bukan validasi schema Actions (`actionlint` tidak tersedia). Root guard hanya diuji lewat fakeroot, bukan instalasi/ownership sebagai root nyata. Tidak menguji crash/power loss, race konkuren, seluruh distro/glibc atau OS lain. Instalasi atomik per file, bukan transaksi/rollback seluruh bundle. Checksum bukan signature/autentikasi; bundle harus dipercaya sebelum installer mengeksekusi binary untuk setup config. Lisensi proyek dan publikasi tetap belum diputuskan.

## Belum diprioritaskan

LSP/autocomplete, debugger, Git UI, plugin, terminal tertanam, crash recovery, soft wrap, dan dukungan encoding tambahan perlu keputusan terpisah. Daftar ini bukan janji fitur atau arah menuju IDE; usulan baru harus tetap mendukung produk editor teks.
