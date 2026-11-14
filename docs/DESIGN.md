# Desain dan Interaksi Notefart

Status: editor hybrid panel + tab, shared document/history, explorer, Find/Replace, highlighting, tema dan ikon tersedia. Batch polish pertama merapikan fokus, status/helper, dialog dan field. **Visual belum final**: proporsi dan estetika tetap dapat disesuaikan berdasarkan penggunaan harian.

## Prinsip visual

- Inspirasi superfile: hierarki visual jelas, spacing rapi, fokus mudah dikenali.
- Setiap panel utama memakai frame penuh dengan judul/ikon; spacing antarpanel memisahkan kartu tanpa garis divider rangkap.
- Panel aktif dibedakan dengan lebih dari warna: label/penanda fokus dan gaya border.
- Tema harus tetap terbaca tanpa truecolor; jangan mengandalkan ikon font khusus untuk fungsi penting.
- Konten dokumen tidak boleh menyamar sebagai status atau dialog aplikasi.

## Identitas visual

**Arah usulan: tenang, compact, dan sedikit playful lewat nama Notefart, bukan lewat dekorasi berlebihan.** Area teks tetap dominan. Tidak memakai gradient, emoji dekoratif, banner besar, atau ikon tanpa label untuk aksi penting.

- Tema utama: **Notefart Dark**, charcoal kebiruan dengan aksen biru lembut.
- Alternatif: **Notefart Light**, putih dingin dengan aksen biru yang lebih gelap.
- Mode kompatibilitas: **Terminal**, memakai default foreground/background terminal dan warna ANSI semantik.
- Dark menjadi default runtime; Light/Terminal dipilih eksplisit. Tidak menebak tema dari waktu atau mendeteksi background semua terminal.
- Palet UI dan syntax menggunakan token semantik yang sama di seluruh aplikasi, bukan warna hardcoded per widget.

## Tema dan token warna

Nilai hex berikut adalah target truecolor, bukan jaminan warna yang terlihat pada seluruh terminal. Rasio kontras pasangan teks/background yang digunakan ditargetkan minimal 4.5:1; border struktural minimal 3:1 terhadap surface terkait. Target ini bukan klaim sertifikasi aksesibilitas.

| Token | Notefart Dark | Notefart Light | Fungsi |
| --- | --- | --- | --- |
| `canvas` | `#171B24` | `#F5F7FB` | Background dasar seragam untuk header, explorer, tab, editor, gutter, gap, status, helper dan dialog |
| `fg` | `#DCE3EE` | `#202A3A` | Teks utama |
| `muted` | `#A4AFC0` | `#536176` | Path sekunder, komentar, nomor baris |
| `border` | `#74829A` | `#69778B` | Pembatas panel nonaktif |
| `accent` | `#8BC5FF` | `#195CA8` | Fokus, shortcut, nama fungsi |
| `selection` | `#344866` | `#C7DCF6` | Background seleksi dengan teks `fg` |
| `success` | `#91D5A5` | `#22623C` | Hasil operasi berhasil |
| `warning` | `#F2CC8F` | `#805300` | Dirty, peringatan, hasil pencarian aktif |
| `error` | `#FF9EAE` | `#A52840` | Error dan aksi berisiko |
| `syntax.keyword` | `#C9A7F5` | `#6D3BA3` | Keyword bahasa |
| `syntax.string` | `#91D5A5` | `#22623C` | String |
| `syntax.number` | `#F2CC8F` | `#8B4B12` | Angka dan literal numerik |
| `syntax.function` | `#8BC5FF` | `#195CA8` | Fungsi |
| `syntax.type` | `#8BD5CA` | `#0A656B` | Tipe |

Komentar memakai `muted`; operator, punctuation, dan identifier tanpa kategori memakai `fg`. Boolean/constant dapat memakai `syntax.number`. Mapping kategori mengikuti engine highlighting yang dipilih, tanpa memaksakan kategori yang tidak disediakan engine.

Markdown memakai kategori markup tersendiri dengan warna palet yang sudah ada:

| Kategori markup | Palet warna |
| --- | --- |
| Heading | `syntax.function` |
| Bold / italic | `syntax.keyword` |
| Inline code / fenced code / indented code | `syntax.string` |
| Link / image / blockquote | `syntax.type` |
| Bullet / nomor list / thematic break | `syntax.number` |

Teks paragraf dan isi list biasa tetap `fg`; scope dalam yang lebih spesifik dapat mengalahkan scope luar, misalnya inline code dalam quote. Ini pewarnaan teks sumber, bukan preview Markdown: delimiter tetap terlihat, bold/italic diberi warna tanpa modifier font tambahan, dan code fence memakai warna raw tanpa embedded highlighting per bahasa. Cakupan mengikuti scope grammar; untuk heading Setext, grammar mewarnai underline tetapi tidak memberi scope heading secara retroaktif pada baris judul sebelumnya. Mode `none` tetap tidak mengirim warna.

### Penerapan ke komponen dan state

| Komponen/state | Aturan visual |
| --- | --- |
| Panel aktif | Border `accent` dan marker judul `●` (`*` pada ASCII); panel lain memakai `border` |
| Tab aktif | Background `canvas`; label/marker dan bold membedakan fokus tanpa bidang warna tambahan |
| Tab nonaktif | Background `canvas`, teks `muted`; dirty tetap diberi penanda |
| Dokumen dirty | Penanda `*` berwarna `warning`, tidak mengganti tombol close |
| Explorer terpilih | Background `selection`, teks `fg`; fokus ditambah penanda `>` |
| Baris cursor | Background tetap `canvas`; nomor baris aktif memakai `accent` |
| Seleksi teks | Background `selection`, foreground `fg`; mengalahkan syntax foreground |
| Hasil pencarian biasa | Belum ditandai semua sekaligus |
| Hasil pencarian aktif | Seleksi editor; pindah hasil memperbarui range dan viewport |
| Error | Label `ERROR`/`!` dan teks `error`, bukan warna merah saja |
| Dialog | Background `canvas`, border `accent`, judul dan aksi berlabel |
| Aksi dialog | Label shortcut `accent` dan bold; hit region hanya pada aksi yang terlihat |
| Status/helper | Background `canvas`; helper memakai kelompok keycap dan label terbaca, bukan seluruh baris bold/berwarna |

Prioritas runtime: seleksi (termasuk hasil Find aktif) > syntax > teks biasa; background baris cursor tidak menimpa seleksi. Fokus/dirty tetap terlihat ketika warna dimatikan. Dimensi komponen tidak berubah karena bold/warna.

Cursor editor mengikuti bentuk terminal pada rancangan awal; jangan mengandalkan kontrol warna/bentuk cursor yang tidak seragam antar terminal. Saat dialog mengambil fokus, jangan meninggalkan cursor seolah masih aktif di dokumen.

### Degradasi warna

- Truecolor: gunakan nilai RGB di atas.
- 256 warna: kuantisasi token ke palet xterm-256, lalu cek ulang pasangan teks/background; jangan menganggap kontras RGB tetap berlaku setelah kuantisasi.
- 16 warna: gunakan default foreground/background, blue/cyan untuk accent, green untuk success, yellow untuk warning, dan red untuk error. Palet aktual dikendalikan terminal.
- Tanpa warna: default foreground/background, reverse untuk seleksi/fokus, serta marker teks. Jika bold/underline tidak tersedia, label dan marker tetap menyampaikan state.
- Sediakan override kemampuan warna; deteksi environment hanya petunjuk, bukan kepastian.
- Background solid menjadi default pada tema Dark/Light. Transparansi bukan default karena background di belakang terminal mengubah kontras. Mode Terminal mengikuti background terminal tanpa menjanjikan transparansi tertentu.

## Font dan tipografi terminal

**Notefart tidak memasang atau mengganti font terminal.** Ratatui menggambar sel dan style; font family, ukuran, ligature, dan line height diatur pengguna melalui terminal.

| Kebutuhan | Rekomendasi usulan |
| --- | --- |
| Font utama dengan ikon | **JetBrainsMono Nerd Font Mono** |
| Alternatif dengan ikon | **Iosevka Nerd Font Mono** |
| Tanpa patched font | JetBrains Mono, Iosevka, atau monospace bawaan terminal; gunakan ikon Unicode/ASCII |
| Ukuran awal untuk dicoba | 11–13 pt; sesuaikan DPI dan kenyamanan, bukan persyaratan aplikasi |
| Line height awal | Sekitar 1.1–1.2 jika terminal menyediakan pengaturan tersebut |
| Ligature | Opsional; nonaktif selama validasi awal mapping cursor/seleksi |
| Weight | Regular untuk isi; bold untuk ikon, judul, tab aktif dan fokus |

Tidak ada ukuran font berbeda untuk judul versus isi di dalam TUI biasa. Hierarki dibuat lewat spacing sel, warna, weight, dan border. Italic tidak menjadi syarat keterbacaan karena dukungannya berbeda-beda.

Pilih varian **Nerd Font Mono**, bukan varian Propo, untuk mengurangi risiko ikon melebar ke sel tetangga. Ini tetap perlu dibuktikan pada terminal/font nyata: metrik font dan lebar yang diasumsikan aplikasi bisa berbeda.

## Ikon dan fallback

Tiga profil runtime: `unicode` bawaan tanpa font khusus, `nerd` opt-in dan `ascii` untuk kompatibilitas. Tidak ada deteksi otomatis Nerd Font. Pilihan dapat dipersistenkan sekali lewat `cargo run -- --icons nerd --save-config`; startup biasa membaca preferensi dan flag CLI dapat mengoverride tanpa menyimpan ulang.

Profil Nerd memakai ikon **Material Design solid** dengan mapping diverifikasi terhadap metadata upstream [glyphnames.json](https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/glyphnames.json). Contoh: folder U+F024B, folder-open U+F0770, file-document U+F0219, file-code U+F022E, link U+F0337 dan close U+F0156. Gunakan Nerd Font Mono terkini, direkomendasikan v3+; [catatan migrasi upstream](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.0.0) menjelaskan rentang Material Design lima digit. Ikon juga memakai modifier bold; verifikasi metadata/lebar sel bukan jaminan bentuk/weight pada semua terminal/font.

| Peran | Ikon Nerd Font yang dicari | Unicode default | ASCII |
| --- | --- | --- | --- |
| Folder tertutup | Folder | `▸ ■` | `> [D]` |
| Folder terbuka | Folder opened | `▾ □` | `v [D]` |
| File teks | File text | `≡` | `[F]` |
| File source | File code | `λ` | `[C]` |
| Konfigurasi | Settings/file config | `⌘` | `[=]` |
| Gambar/biner | File media/binary | `▣` / `※` | `[B]` |
| Tutup tab | Close | `×` | `x` |
| Dokumen dirty | Tanpa ikon font khusus | `*` | `*` |
| Read-only | Lock | `[RO]` | `[RO]` |
| Symlink | Link | `↪` | `@` |
| Error/peringatan | Error/warning | `!` + label | `!` + label |

- Ikon file selalu disertai nama; profil tanpa Nerd Font tidak kehilangan fungsi.
- Chevron folder memiliki slot sendiri; saat ini seluruh baris folder men-toggle ekspansi, bukan hit target chevron terpisah.
- Pemetaan awal cukup kategori teks, source, konfigurasi, dan biner. Ikon/logo per bahasa bukan syarat awal; file tak dikenal memakai ikon file generik.
- Jangan memakai emoji folder/file sebagai default: presentation dan lebar sel dapat berbeda.
- Slot ikon file Unicode/Nerd dua sel, ASCII tiga sel, lalu satu sel pemisah sebelum label. Chevron dan badge memakai slot terpisah; hit testing mengacu ukuran sel, bukan jumlah karakter string.
- File tak dikenal memakai ikon generik `▫` pada Unicode; ikon tab mengikuti kategori file yang sama dengan explorer. Warna ikon semantik tidak menimpa style baris terseleksi.
- Pengguna harus bisa mengganti ke profil ASCII jika glyph kosong, terpotong, atau tidak sejajar. Fallback ini tidak mengubah nama/path asli.
- Explorer dan setiap panel editor memakai frame penuh membulat, dipisahkan gap drag satu sel. Header dan status berbingkai saat ruang cukup; helper adalah baris biasa terpisah di bawah frame status. ASCII memakai `+`, `-`, dan `|`. Compact mengurangi chrome global, bukan menghapus border panel.
- Label/path dipotong pada batas grapheme; Unicode memakai `…`, ASCII `...` (dipendekkan bila anggaran kurang dari tiga sel). ASCII juga memakai border/separator ASCII, tanpa mengganti teks/nama file Unicode. Aksi informasi path lengkap masih target, bukan tooltip yang sudah tersedia.

## Ukuran dan ritme layout

Semua ukuran aplikasi memakai **kolom/baris terminal**, bukan pixel. Ukuran berikut sudah digunakan runtime; dukungan emulator/font tetap mengikuti bukti pengujian.

| Elemen | Ukuran runtime |
| --- | --- |
| Explorer | 28 kolom termasuk frame; resize 18–40 selama ruang editor cukup |
| Frame editor | Minimum max(12, digit gutter + 5) kolom ×4 baris, termasuk border, tab dan satu baris teks |
| Tab bar per panel | 1 baris di dalam frame; sisakan kontrol overflow saat tab tidak muat |
| Tab | Ikon kategori, label, dirty dan close; label panjang dipotong |
| Header / dock normal | 3 / 4 baris tanpa gap vertikal luar; dock terdiri dari frame status 3 baris dan helper tanpa frame 1 baris |
| Header / dock compact | 1 / 1–2 baris, tanpa gap global; minimum editor tunggal 12×6 |
| Divider split | Gap 1 kolom/baris dengan grip tengah; kedua panel tetap punya frame sendiri |
| Padding explorer/dialog | 1 sel horizontal; dialog dapat memakai 1 baris vertikal |
| Gutter nomor baris | Digit jumlah baris dokumen, minimum 2 digit, ditambah 1 kolom pemisah |

Ukuran tab minimum, slot ikon, border, dan padding masuk perhitungan hit target. Layout compact tidak boleh membuat tombol close bertumpuk dengan label atau menyembunyikan dirty marker. Dialog tidak boleh mengandalkan bayangan/transparansi yang tidak konsisten di terminal.

## Pemeriksaan visual sebelum desain dianggap siap

- Tampilkan layout yang sama dalam Dark, Light, Terminal, dan tanpa warna.
- Periksa editor, explorer, tab dirty, read-only, error, dialog, seleksi, dan hasil pencarian; bukan hanya layar kosong.
- Hitung kontras pasangan yang benar-benar dipakai, termasuk foreground `canvas` di atas `accent`/`warning`; cek ulang fallback 256 warna.
- Uji Nerd Font Mono, font biasa dengan Unicode, dan profil ASCII; cek baseline, lebar glyph, clipping, serta posisi klik.
- Coba terminal kecil dan resize ketika tab panjang, dialog, atau split aktif.
- Ambil bukti tampilan aplikasi nyata setelah runtime tersedia. Palet dan tabel ini adalah spesifikasi, bukan bukti UI sudah dirender.


## Struktur layout

| Area | Isi dan perilaku |
| --- | --- |
| Explorer kiri | Root browsing aktual, parent `..`, tree expandable dan file; Enter masuk folder, Backspace naik; dapat disembunyikan/di-resize |
| Area editor | Split horizontal/vertikal, dengan tab bar masing-masing panel |
| Gutter editor | Nomor baris dan ruang penanda; bukan bagian teks yang disalin |
| Status bar | Pesan operasi/path serta dirty/read-only, posisi cursor, newline dan UTF-8/BOM; metadata menyusut saat ruang sempit |
| Helper bawah | Kelompok `[tombol] Aksi` sesuai fokus, dipisahkan `·` (`|` pada ASCII); hanya grup lengkap yang terlihat dapat diklik |
| Dialog/overlay | Open path, Save As, pencarian/penggantian, konfirmasi, dan error |

Tanpa argumen: tab kosong dan explorer direktori kerja. Argumen folder menentukan root; argumen file membukanya dengan direktori induk sebagai root awal.

## Fokus dan pembukaan file

- Klik panel editor menjadikannya panel tujuan terakhir.
- Fokus explorer tidak menghapus tujuan tersebut; membuka file diarahkan ke panel tujuan terakhir yang masih ada.
- Membuka file baru langsung membuat tab biasa; tidak ada preview tab yang mudah terganti pada rancangan awal.
- Membuka dokumen yang sudah menjadi tab di panel tujuan mengaktifkan tab itu. Jika hanya terbuka di panel lain, buat view baru atas dokumen yang sama.
- Semua area dan aksi mouse memiliki jalur keyboard.
- Modal menangkap input sampai selesai atau dibatalkan. Menutup modal mengembalikan fokus sebelumnya jika masih valid.

## Mouse

| Aksi | Perilaku yang diusulkan |
| --- | --- |
| Klik folder | Expand/collapse tanpa berganti panel tujuan |
| Klik file | Buka/aktifkan tab di panel tujuan |
| Klik tab / tombol × | Aktifkan / minta penutupan tab |
| Klik teks | Fokus editor dan tempatkan cursor pada posisi teks terdekat yang valid |
| Drag teks | Seleksi dari anchor; autoscroll ketika melewati tepi viewport |
| Double-click kata | Pilih kata menurut aturan boundary yang konsisten dengan navigasi kata |
| Wheel | Scroll area di bawah pointer tanpa memindahkan fokus keyboard |
| Drag pembatas | Resize explorer atau split dengan minimum ukuran area |

Klik pada gutter, border, status, atau helper tidak boleh menyisipkan teks. Drag aktif tetap dimiliki area asal sampai mouse dilepas. Perubahan layout membatalkan hit target lama.

Seleksi bawaan terminal dapat berbenturan dengan mouse capture. Bypass memakai Shift tersedia di sejumlah terminal, bukan jaminan universal. Dokumentasi dukungan terminal harus menjelaskan hasil pengujian nyata.

## Shortcut runtime

| Shortcut | Aksi |
| --- | --- |
| F1 | Buka bantuan; Esc/F1 kembali ke konteks editor/explorer/modal sebelumnya |
| Ctrl+S | Save dokumen aktif; dokumen tanpa path membuka Save As |
| Ctrl+O | Dialog Open path, menambah/mengaktifkan tab |
| Ctrl+N | Tab dokumen baru |
| Ctrl+Z / Ctrl+Y | Undo / redo pada dokumen aktif |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste saat fokus di editor |
| Ctrl+A / Ctrl+U | Pilih seluruh teks sesuai fokus / kosongkan field dialog |
| Shift+panah | Perluas seleksi |
| Ctrl+F / Ctrl+R | Find / Replace literal dalam dokumen aktif |
| Ctrl+W | Tutup tab aktif |
| Ctrl+Q | Minta keluar aplikasi |
| Esc | Batalkan dialog atau mode interaksi sementara, bukan keluar aplikasi |
| F4 | Save As |
| Alt+Left/Right atau Ctrl+PageUp/Down | Pindah tab dalam panel aktif |
| Ctrl+B / F2 | Toggle explorer / pindah fokus explorer-editor |
| Right/Left, Enter, F5 di explorer | Expand/collapse, buka/toggle, refresh |
| Enter/F3 / Shift+F3 | Hasil berikutnya / sebelumnya dengan wrap |
| F6 / F7 dalam Replace | Ganti hasil aktif / Replace All satu undo |
| F8 / F9 | Split kiri-kanan / atas-bawah |
| F10 / Shift+F10 | Fokus panel berikutnya / sebelumnya, termasuk yang sementara tersembunyi |
| F12 | Tutup panel dengan guard per dokumen |
| Alt+Shift+Left/Right atau Up/Down | Geser divider Columns terdekat dua sel / Rows satu sel |

Ctrl+C tanpa seleksi tidak keluar atau menyalin seluruh dokumen. Modal mengambil input field, bukan meneruskan shortcut editor ke dokumen belakang. Ctrl+H tetap Backspace karena byte terminal sama; Replace memakai Ctrl+R.

Helper memprioritaskan aksi inti, bukan seluruh shortcut split. F1 dan `--help` memakai katalog berkelompok yang sama. Bantuan memakai panah, PageUp/Down, Home/End dan wheel; page mengikuti jumlah baris visual yang benar-benar terlihat. Flow control, font dan protokol terminal masih memerlukan matriks validasi lebih luas.

## Siklus hidup panel dan dokumen

- Membuat split menampilkan dokumen aktif dengan view independen; teks tetap shared.
- Menutup panel yang memiliki beberapa tab memeriksa dokumen yang akan kehilangan view terakhir.
- Menutup view dokumen yang masih terlihat di panel lain tidak membuang dokumen tersebut.
- Penutupan view terakhir dokumen dirty: Save, Discard, atau Cancel.
- Save gagal atau dibatalkan berarti penutupan/exit dibatalkan.
- Dialog keluar dengan beberapa dokumen dirty harus menjelaskan pilihan per dokumen; jangan menafsirkan Discard satu dokumen sebagai Discard All.
- Panel terakhir tidak hilang menjadi layar buntu; usulan: ganti tab terakhir yang ditutup dengan dokumen kosong.

## Resize dan kondisi terbatas

- Pertahankan rasio split sejauh ukuran minimum memungkinkan.
- Jika layout tidak muat, sembunyikan sementara helper/explorer lalu tampilkan hanya panel aktif bila perlu; struktur panel dan buffer tidak dihapus.
- Jika area editor minimum tetap tidak muat, tampilkan pesan terminal terlalu kecil dan tetap sediakan jalur keluar yang aman.
- Mengembalikan ukuran terminal memulihkan layout tanpa kehilangan state editing.

## Error dan status

Pesan error menyebut operasi, target, dan tindakan yang bisa dilakukan, tanpa menyembunyikan kegagalan. Bedakan “tersalin ke clipboard internal” dengan “tersalin ke clipboard sistem”. File unsupported tampil dengan alasan; bukan buffer kosong yang tampak berhasil dimuat.

Pesan status yang muat tidak dibatasi setengah baris hanya untuk menyisakan path. Error copy/cut/paste termasuk status error; error dan warning mendahulukan pesan daripada path serta metadata cursor/format yang opsional. Jika tetap melampaui lebar terminal, elisi tengah mempertahankan awal aksi dan akhir penyebab pada batas grapheme, memakai `…` atau `...` sesuai profil. Metadata tidak dihitung ketika pesan kritikal jelas mengambil seluruh baris. Tinggi dock/helper dan area klik tidak berubah; ini bukan viewer pesan lengkap, sehingga terminal sangat kecil tetap membatasi informasi.

## Realisasi workspace dan batas visual

- Header/explorer global, split dua orientasi rekursif dan status/helper/dialog global tersedia. Explorer dan setiap editor memiliki frame penuh dengan judul/ikon; tab/gutter/body berada di dalamnya. Fokus memakai border accent/bold serta `●` atau `*` pada ASCII, bukan warna saja. Tab/close/overflow memakai view ID stabil.
- Dark/Light/Terminal, auto/truecolor/256/16/none dan Unicode/Nerd/ASCII tersedia melalui CLI. Kontras RGB/xterm-256 diuji; ANSI16/Terminal dikendalikan palet pengguna. Tanpa warna tetap memakai marker, reverse dan bold.
- Find/Replace literal case-sensitive dengan range hasil terseleksi, wrap, F6 tervalidasi dan F7 satu transaksi undo. Tidak ada regex, case folding atau marker semua hasil. Syntax Rust/Python/JSON dan konteks multiline dibuktikan; parser tidak berjalan dalam draw.
- Ctrl+N/O menargetkan panel terakhir; dokumen yang sama memakai satu registry/history lintas panel. Ctrl+W menutup view dan hanya meminta guard saat view terakhir dirty. F12 memeriksa semua dokumen final dalam panel; Ctrl+Q memeriksa dirty sekali per dokumen. Save gagal/cancel menghentikan continuation.
- Ctrl+S/F4, konflik eksternal dan metadata save tetap memakai kebijakan keamanan. Field path/search/replacement memiliki caret grapheme, Shift selection, Ctrl+A/U, Backspace/Delete, routing clipboard backend terpilih dan klik/drag dengan window mengikuti caret. Ctrl+Z/Y modal tidak mencapai dokumen belakang. CLI mendukung path non-UTF-8.
- Mouse membuka file/folder, mengaktifkan/menutup tab di panel yang benar, me-resize explorer/split dan memilih teks. Border hanya memfokuskan panel; gutter tetap menavigasi baris. Wheel bekerja di seluruh kartu tanpa mengganti fokus; drag lintas panel tetap dimiliki view asal. Scroll keyboard/autoscroll serta edit menginvalidasi hit stale hingga frame baru tanpa membuang snapshot layout.
- Minimum tunggal compact 12×6; tiap panel minimum lebar max(12, digit gutter + 5), tinggi 4 baris termasuk border/tab/teks. Chrome global dipadatkan sebelum menyembunyikan panel. Split ditolak jika minimum tree baru tidak muat dengan chrome compact tanpa explorer. Resize dapat menyembunyikan explorer/panel lain sementara, tanpa mengubah ratio/tree/view. Modal umum minimum 20×6; search 40×10.
- Paste valid satu transaksi, CRLF/CR menjadi LF. Paste >1 MiB/invalid UTF-8 ditolak utuh. Explorer maksimum 10.000 entri/direktori; semua field 4096 byte UTF-8 sesudah normalisasi/replacement; highlight >16 KiB/baris berhenti dengan warning dan plaintext.

Redesign terbaru lolos format, 217 unit test dan build. Bukti PTY mencakup nested frame, default Unicode/Nerd/ASCII, empat profil tema/warna, klik border/gutter, wheel di seluruh kartu, drag gap, paging, tab close, minimum 12×6 dan compact/collapse/restore. Grid terminal nyata juga diraster dengan font Nerd Mono sementara untuk inspeksi komposisi; ini bukan validasi font pada emulator fisik. SSH/emulator lain dan clipboard desktop belum diklaim terverifikasi. Detail di [README.md](README.md) dan [ROADMAP.md](ROADMAP.md).

## Interaksi split/shared-view yang tersedia

- F8 split kiri/kanan, F9 atas/bawah; panel baru menampilkan view independen dokumen aktif. F10/Shift+F10 berpindah panel, F12 menutup panel dengan guard per dokumen.
- Alt+Shift+Left/Right menggeser divider Columns terdekat dua sel; Alt+Shift+Up/Down divider Rows terdekat satu sel. Alt+Left/Right tetap pindah tab dalam panel.
- Klik/fokus panel menetapkan tujuan Open/New/explorer. Wheel memakai panel di bawah pointer tanpa memindahkan fokus; drag seleksi tetap milik panel asal.
- Batas jumlah panel mengikuti minimum ruang aktual. Split ditolak jika tree baru tidak muat bahkan tanpa explorer; layar kecil sementara menampilkan panel aktif saja dan restore tree/ratio setelah diperbesar.
- Menutup tab terakhir panel non-terakhir menutup panel; panel terakhir mendapat tab kosong. Menutup satu view dokumen dirty yang masih memiliki view lain tidak meminta discard. Close panel memeriksa seluruh dokumen yang kehilangan view terakhir.
- Struktur ini belum desain visual final. Pemolesan awal tercatat di bawah; spacing, proporsi dan estetika keseluruhan tetap dapat ditinjau melalui penggunaan nyata.


## Hasil batch pemolesan UI/UX

- Batch awal memakai marker fokus `[>]` serta footer dengan aksi inti. Redesign berikutnya mengganti marker tersebut dengan judul frame dan indikator fokus; detail split dan shortcut lain tetap tersedia melalui F1.
- Status menerima metadata path/format/dirty terpisah dari pesan operasi. Posisi dan format tersedia saat muat; error mendapat prioritas ruang. Dialog berjudul singkat dan tidak mengulang hint di judul.
- Field path/search/replacement dapat dinavigasi dan diseleksi per grapheme, dengan clipboard backend terpilih serta klik/drag berbasis byte sumber. Normal text dipinjam; kontrol/Bidi dan cluster zero-width standalone ditampilkan aman. Grapheme lebar tidak dipecah menjadi byte terminal.
- F1 kembali ke modal semula tanpa membatalkan isi, seleksi atau pending save/close. Bantuan di-scroll pada 20×6; offset/page memakai wrapping aktual dan tetap benar sesudah resize.
- PTY membuktikan Dark/truecolor/ASCII, Light/256/Unicode, Terminal/none/ASCII dan Dark/16/Nerd, termasuk seleksi field, metadata CRLF+BOM, escape OSC/Bidi dan restorasi terminal. Ini bukti protokol/cell, bukan validasi glyph/font fisik.
- Batch ini tidak menutup seluruh tahap kesiapan rilis atau mengunci desain visual final.


## Hasil redesign bergaya superfile

- Permintaan pengguna: perubahan visual menyeluruh, bukan polish minor. Acuan komposisi adalah [superfile](https://superfile.dev/): kartu panel berbingkai, ikon, judul terintegrasi dan fokus yang tegas; bukan menyalin fitur file manager yang tidak tersedia.
- Explorer dan setiap editor/split mendapat frame penuh. Tab berada di dalam frame dan membawa ikon file. Border tidak menjadi posisi dokumen; gutter tetap memetakan navigasi baris. Gap satu sel tetap dapat di-drag.
- Header workspace dan frame status tersedia saat lebar ≥40, tinggi ≥18 dan seluruh tree masih muat setelah overhead 7 baris. Header 3 baris, status 3 baris dan helper biasa 1 baris langsung berdampingan dengan body tanpa gap kosong. Compact mengurangi chrome global dahulu; minimum panel 12×4 dan editor tunggal compact 12×6 tetap.
- Unicode default memakai ikon grafis, bukan badge `[F]`/`[D]`; Nerd tetap opt-in untuk font yang sesuai dan ASCII tetap tersedia. Warna ikon semantik tidak menimpa style seleksi.
- Geometry berada di modul `chrome`, dipakai renderer dan controller bersama agar split preflight, scroll, klik, drag dan resize tidak memakai offset lama.
- Format, 217 unit test, build dan skenario PTY lolos. Gap global memakai warna canvas eksplisit pada Dark/Light, bukan background terminal bawaan. Glyph explorer Unicode dipilih agar lebar selnya konsisten pada library aplikasi dan parser terminal yang diperiksa. Desain ini tersedia untuk evaluasi pengguna, bukan pernyataan seluruh tahap kesiapan rilis selesai.

## Hasil penyesuaian helper, ikon dan browser folder

- Helper menjadi baris terpisah tanpa frame/label Keys, memakai kelompok `[tombol] Aksi` dengan separator titik; label normal/muted dan keycap tegas. Shortcut aktual editor tidak diganti menjadi huruf tunggal dari referensi.
- Background dasar header, explorer, editor, gutter, tab, gap dan status diseragamkan; hanya seleksi/fokus kontrol memakai penekanan semantik. Tidak ada strip background khusus baris cursor.
- Ikon Nerd memakai keluarga Material Design solid dan modifier bold; Unicode/ASCII juga bold tanpa mengganti font terminal pengguna. Hasil bentuk/weight bergantung font dan terminal. Profil Unicode/Nerd/ASCII tetap ada.
- Explorer menjadi browser folder dengan baris `..` di atas tree dan Backspace untuk naik. Enter membuka folder sebagai root browsing baru; Left/Right tetap collapse/expand tree, klik folder tetap toggle. Perubahan root tidak menutup dokumen atau mengubah working directory proses.
- Naik di filesystem root tidak melakukan apa-apa; kegagalan listing mempertahankan browser sebelumnya. Folder symlink tidak ditelusuri otomatis. Belum ada history Back/Forward atau operasi file manager.
- Folder `testfile` berisi tujuh contoh yang tidak dieksekusi aplikasi. YAML/YML, JS, HTML dan CSS tetap ter-highlight. Batch berikutnya menambahkan grammar TypeScript/TSX asli, termasuk JSX, sehingga kedua contoh tidak lagi plaintext atau alias parser JavaScript.

- PTY membuktikan parent lewat keyboard/helper/mouse, Enter, folder kosong, root `/`, refresh, wheel baris parent, no-follow saat directory berubah menjadi symlink, serta shared save/undo dengan CWD tetap. Empat profil tema/warna/ikon menjaga alignment, bold ikon, label helper non-bold dan modal isolation.
- Frame/cell dari aplikasi nyata diraster dengan font Nerd Mono sementara untuk inspeksi komposisi CSS/JS/YAML. Font/config sistem tidak diubah. Bukti unit/build dan batas dukungan berada di ROADMAP.md.
