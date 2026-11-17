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

- Tema utama: **Notefart Dark**, foreground terang dengan aksen biru lembut untuk terminal berlatar gelap.
- Alternatif: **Notefart Light**, foreground gelap dengan aksen biru yang lebih gelap untuk terminal berlatar terang.
- Mode kompatibilitas: **Terminal**, memakai default foreground/background terminal dan warna ANSI semantik.
- Dark menjadi default runtime; Light/Terminal dipilih eksplisit. Tidak menebak tema dari waktu atau mendeteksi background semua terminal.
- Palet UI dan syntax menggunakan token semantik yang sama di seluruh aplikasi, bukan warna hardcoded per widget.

## Tema dan token warna

Nilai hex berikut berlaku untuk foreground dan highlight truecolor; canvas memakai background default terminal. Pasangan foreground/background yang dikendalikan aplikasi (seleksi, tab aktif, prefix field fokus) ditargetkan memiliki rasio kontras minimal 4.5:1. Kontras teks dan border terhadap canvas bergantung pada konfigurasi terminal serta latar di belakang efek glass; ini bukan klaim sertifikasi aksesibilitas.

| Token | Notefart Dark | Notefart Light | Fungsi |
| --- | --- | --- | --- |
| `canvas` | Default terminal | Default terminal | `Color::Reset` untuk header, explorer, tab nonaktif, editor, gutter, gap, status, helper dan dialog |
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
| Tab aktif | Background `selection`, foreground `fg` untuk label, ikon, dirty, read-only dan close; marker fokus dan bold tetap membedakan panel aktif |
| Tab nonaktif | Background default terminal, teks `muted`; dirty tetap diberi penanda `warning` |
| Dokumen dirty | Penanda `*` memakai `warning` pada tab nonaktif dan foreground seleksi pada tab aktif; tidak mengganti tombol close |
| Explorer terpilih | Background `selection`, teks `fg`; fokus ditambah penanda `>` |
| Baris cursor | Background tetap `canvas`; nomor baris aktif memakai `accent` |
| Seleksi teks | Background `selection`, foreground `fg`; mengalahkan syntax foreground |
| Hasil pencarian biasa | Belum ditandai semua sekaligus |
| Hasil pencarian aktif | Seleksi editor; pindah hasil memperbarui range dan viewport |
| Error | Label `ERROR`/`!` dan teks `error`, bukan warna merah saja |
| Dialog | Background `canvas`, border `accent`, judul dan aksi berlabel |
| Aksi dialog | Label shortcut `accent` dan bold; hit region hanya pada aksi yang terlihat |
| Status/helper | Background `canvas`; helper memakai kelompok keycap dan label terbaca, bukan seluruh baris bold/berwarna |
| Prefix field fokus | Pasangan `selection`/`fg` dengan bold; tidak memakai warna canvas sebagai foreground |

Prioritas runtime: seleksi (termasuk hasil Find aktif) > syntax > teks biasa; background baris cursor tidak menimpa seleksi. Fokus/dirty tetap terlihat ketika warna dimatikan. Dimensi komponen tidak berubah karena bold/warna.

Pada baris explorer terpilih, marker fokus `>` dan indikator issue `!` memakai foreground seleksi yang terbaca, bukan memaksakan accent/warning yang gagal target kontras. Bentuk marker dan bold tetap membedakan fungsinya. Border panel aktif dan indikator issue nonselected tetap memakai token accent dan warning secara eksplisit; header compact mengisi seluruh canvas baris.

Cursor editor mengikuti bentuk terminal pada rancangan awal; jangan mengandalkan kontrol warna/bentuk cursor yang tidak seragam antar terminal. Saat dialog mengambil fokus, jangan meninggalkan cursor seolah masih aktif di dokumen.

### Degradasi warna

- Truecolor: gunakan nilai RGB di atas.
- 256 warna: kuantisasi token ke palet xterm-256, lalu cek ulang pasangan teks/background; jangan menganggap kontras RGB tetap berlaku setelah kuantisasi.
- 16 warna: gunakan default foreground/background, blue/cyan untuk accent, green untuk success, yellow untuk warning, dan red untuk error. Palet aktual dikendalikan terminal.
- Tanpa warna: default foreground/background, reverse untuk seleksi/fokus, serta marker teks. Jika bold/underline tidak tersedia, label dan marker tetap menyampaikan state.
- Sediakan override kemampuan warna; deteksi environment hanya petunjuk, bukan kepastian.
- Semua tema mengikuti background default terminal; aplikasi tidak melukis bidang canvas solid atau mengatur opacity/blur. Tab aktif, seleksi dan fokus kontrol tetap memakai highlight semantik. Pilih Dark/Light sesuai latar terminal atau Terminal untuk foreground default; kontras terhadap wallpaper/transparansi tidak dapat dijamin aplikasi.

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

## Kontrak batch P1 aksesibilitas dan pencarian

Kontrak batch yang disetujui telah terintegrasi dan lulus validasi fungsional DeepSeek pada aplikasi nyata; metode dan batas dicatat di [ROADMAP](ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian):

- Pada helper editor 80 kolom, akses F1 Help/All, Quit, Files dan Pane tetap terlihat serta dapat diklik; aksi lain boleh disembunyikan dengan penanda akses ke F1. Help/All memakai satu chip F1, bukan dua aksi duplikat. Tidak menambah baris dock.
- Dialog konfirmasi memulai fokus pada Cancel, bukan aksi destruktif. Tab/Right/Down maju dan Shift+Tab/Left/Up mundur secara melingkar; Enter/Space menjalankan tombol fokus. Mnemonic, mouse, Esc serta F1 tetap berlaku; kembali dari F1 mempertahankan fokus. Guard ukuran berlaku sebelum aksi/navigasi.
- Saat explorer fokus dan terlihat, Alt+Shift+Left/Right mengubah lebarnya dua kolom dengan batas yang sama seperti mouse. Saat editor fokus, shortcut tetap menggeser split terdekat.
- Tombol overflow tab yang tampak nonaktif tidak menerbitkan hit klik. Navigasi tab keyboard tetap dapat wrap.
- Ctrl+F/Ctrl+R dari editor mengisi query dari seleksi satu baris yang muat dalam batas 4096 byte; seleksi multiline/terlalu panjang tidak dipotong dan tidak mengubah query sebelumnya. Pergantian mode di dalam dialog tidak mengambil ulang seleksi hasil pencarian.
- Penghitung memakai indeks maksimal 100 awal match grapheme-valid; match berikutnya menandai total `100+`. Posisi di luar prefix ditulis `>100`, bukan angka pasti buatan. Indeks dibuat pada aksi pencarian, dicache menurut dokumen/revisi/query, bukan dipindai setiap frame atau keystroke. Find mengunjungi match overlapping pada kedua arah tanpa batas 100 hasil; Replace All tetap non-overlap. Penanda wrapped mengikuti perpindahan sebenarnya.
- Konfirmasi Unsaved/Overwrite/Conflict menampilkan target path yang disanitasi dan dielisi tengah, atau label Untitled. Konteks target ikut perhitungan kelayakan dialog; layar terlalu kecil tetap hanya menawarkan Resize/Esc.

## Ukuran dan ritme layout

Semua ukuran aplikasi memakai **kolom/baris terminal**, bukan pixel. Ukuran berikut sudah digunakan runtime; dukungan emulator/font tetap mengikuti bukti pengujian.

| Elemen | Ukuran runtime |
| --- | --- |
| Explorer | 28 kolom termasuk frame; resize 18–40 selama ruang editor cukup |
| Frame editor | Minimum max(12, digit gutter + 5) kolom ×4 baris, termasuk border, tab dan satu baris teks |
| Tab bar per panel | 1 baris, diikuti 1 baris kosong sebelum gutter/teks bila tinggi panel ≥5; gap dihilangkan pada minimum 4 baris agar satu baris teks tetap tersedia |
| Tab | Ikon kategori, label, dirty dan close; label panjang dipotong |
| Header / dock normal | 3 / 4 baris tanpa gap vertikal luar; dock terdiri dari frame status 3 baris dan helper tanpa frame 1 baris |
| Header / dock compact | 1 / 1–2 baris, tanpa gap global; minimum editor tunggal 12×6 |
| Divider split | Kiri–kanan berjarak 1 kolom dengan grip; atas–bawah tetap dua frame terpisah pada baris bertetangga, tanpa baris kosong tambahan atau indikator horizontal |
| Padding explorer/dialog | 1 sel horizontal; dialog dapat memakai 1 baris vertikal |
| Gutter nomor baris | Digit jumlah baris dokumen, minimum 2 digit, ditambah 1 kolom pemisah |

Ukuran tab minimum, slot ikon, border, dan padding masuk perhitungan hit target. Layout compact tidak boleh membuat tombol close bertumpuk dengan label atau menyembunyikan dirty marker. Dialog tidak boleh mengandalkan bayangan/transparansi yang tidak konsisten di terminal.

Border tiap panel tetap utuh dan tidak berbagi baris; judul berada pada frame masing-masing. Tidak ada grip horizontal pada semua profil ikon, termasuk ASCII, agar indikator tidak menimpa garis/sudut atau bergantung pada combining glyph. Drag resize atas–bawah tetap melalui border bawah subtree atas; shortcut tidak berubah. Pada rasio sel terminal umum sekitar 2:1, jarak stroke atas–bawah sebanding dengan kiri–kanan; posisi pixel mengikuti font/line height terminal. Dua panel minimum memerlukan body 8 baris.

## Pemeriksaan visual sebelum desain dianggap siap

- Tampilkan layout yang sama dalam Dark, Light, Terminal, dan tanpa warna.
- Periksa editor, explorer, tab dirty, read-only, error, dialog, seleksi, dan hasil pencarian; bukan hanya layar kosong.
- Hitung kontras foreground pada seleksi/tab aktif dan prefix field fokus yang benar-benar dipakai, termasuk fallback 256 warna. Keterbacaan canvas default perlu diperiksa pada konfigurasi terminal pengguna.
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

Tanpa path: pulihkan sesi default, atau tab kosong dan explorer direktori kerja bila belum ada snapshot. Explorer selalu mulai tampil, termasuk bila snapshot terakhir menyembunyikannya; root, lebar dan state editing tetap dipulihkan. `--no-session` tanpa path juga memulai dengan explorer tampil. Argumen folder menentukan root dan menampilkan explorer; argumen file membukanya dengan explorer tersembunyi dan direktori induk sebagai root awal, termasuk file baru yang belum ada. Ctrl+B menampilkan/menyembunyikan explorer tanpa membuang root, lebar atau buffer. Launch file/folder tidak mengubah sesi default; terminal sempit tetap memakai aturan layout compact.

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
| Klik track scrollbar | Page scroll pada sumbu terkait tanpa mengubah fokus, cursor atau seleksi |
| Drag thumb scrollbar | Scroll viewport area asal, termasuk saat pointer keluar track/panel; clamp pada kedua ujung |
| Drag pembatas | Resize explorer atau split dengan minimum ukuran area |

Klik pada gutter, border, status, atau helper tidak boleh menyisipkan teks. Drag aktif tetap dimiliki area asal sampai mouse dilepas atau capture dibatalkan. Key, pergantian konteks/modal dan resize membatalkan capture scrollbar; perubahan layout membatalkan hit target lama.

Seleksi bawaan terminal dapat berbenturan dengan mouse capture. Bypass memakai Shift tersedia di sejumlah terminal, bukan jaminan universal. Dokumentasi dukungan terminal harus menjelaskan hasil pengujian nyata.

Scrollbar editor menempati jalur satu sel di dalam frame, bukan border pembatas split. Jalur tetap dicadangkan saat isi muat; track non-overflow tidak interaktif. Pada panel sangat kecil, jalur yang tidak muat disembunyikan agar tetap ada teks. Warna background mengikuti terminal; bentuk tipis/tebal atau ASCII membedakan rail/thumb pada mode tanpa warna. Lebar horizontal mengikuti baris yang terlihat, termasuk satu sel caret EOL; `?` menandai pengukuran bertahap yang belum selesai, bukan thumb yang dapat diseret. Rincian state dan budget ada di [ARCHITECTURE.md](ARCHITECTURE.md#scrollbar-editor).

Explorer dan F1 memakai scrollbar vertikal dengan gaya yang sama. Rail Explorer hanya sejajar baris tree; parent row dan divider bukan bagian rail. Rail F1 hanya sejajar katalog yang di-wrap, tidak menyempitkan footer Esc/F1. Drag tidak memilih/membuka file atau mengubah field modal yang ditangguhkan; track konten yang muat tidak interaktif. Minimum track dua sel menjaga thumb tetap dapat dipakai. [Kontrak state](ARCHITECTURE.md#scrollbar-explorer-dan-f1).

Explorer yang gagal memuat root tetap menampilkan kartu dengan penyebab dan hint F5 Retry, serta Bksp Up/parent row bila parent diketahui. F2 dapat memfokuskan kartu; helper tidak menawarkan Open tanpa listing atau Up tanpa parent. Pemilihan entri ber-issue menampilkan penyebab di status tanpa perlu membuka file. Mengetik prefix melompat pada row yang sudah dimuat (bukan filter); timeout 900 ms, satu karakter berulang menggilir kecocokan, navigasi/mouse/fokus mereset prefix.

Field path/Find/Replace menampilkan `‹`/`›` saat teks berada di luar window (`<`/`>` pada ASCII, termasuk mode tanpa warna). Jika window dasar menyembunyikan teks dan tersedia minimal tiga sel nilai, dua gutter satu sel dipisahkan dari teks; sisi yang tidak overflow kosong. Indikator tidak dapat diklik untuk memindahkan caret/fokus atau ikut terseleksi; prefix tetap terpisah. Dengan satu/dua sel nilai, gutter tidak dibuat agar caret tetap terlihat. Pemotongan sebagian grapheme tetap memakai marker clipping lama; [kontrak byte/hit](ARCHITECTURE.md#keputusan-pemolesan-field-dan-bantuan).

Field memiliki undo/redo lokal: Ctrl+Z, Ctrl+Y atau Ctrl+Shift+Z memulihkan teks serta cursor/seleksi hanya pada field aktif, termasuk sesudah F1. Ctrl+Backspace/Delete menghapus seleksi atau kata sesuai boundary Ctrl+Left/Right. Drag yang ditahan di luar tepi teks memperluas seleksi dan menggeser window tanpa mengubah isi field; release, key, modal/fokus atau resize menghentikannya. Ctrl+Backspace/Ctrl+Shift+Z memerlukan terminal yang mengirim encoding tombol termodifikasi; byte legacy Ctrl+H tetap Backspace. [Kontrak dan batas history](ARCHITECTURE.md#kontrak-ergonomi-field-dan-navigasi-explorer).

## Shortcut runtime

| Shortcut | Aksi |
| --- | --- |
| F1 | Buka bantuan; Esc/F1 kembali ke konteks editor/explorer/modal sebelumnya |
| Ctrl+S | Save dokumen aktif; dokumen tanpa path membuka Save As |
| Ctrl+O | Dialog Open path, menambah/mengaktifkan tab |
| Ctrl+N | Tab dokumen baru |
| Ctrl+Z / Ctrl+Y | Undo / redo dokumen atau field aktif sesuai fokus; tidak menembus modal |
| Tab di editor | Spasi sampai tab stop 4; bila seleksi melintasi newline, tambah empat spasi pada setiap baris tersentuh |
| Shift+Tab di editor | Kurangi satu tab awal atau hingga empat spasi pada baris aktif/terpilih; satu undo per blok |
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
| Ketik prefix di explorer | Lompat ke nama yang cocok; karakter tunggal berulang menggilir kecocokan dengan wrap |
| Enter/F3 / Shift+F3 | Hasil berikutnya / sebelumnya dengan wrap |
| F6 / F7 dalam Replace | Ganti hasil aktif / Replace All satu undo |
| F8 / F9 | Split kiri-kanan / atas-bawah |
| F10 / Shift+F10 | Fokus panel berikutnya / sebelumnya, termasuk yang sementara tersembunyi |
| F11 / Alt+P | Buka Appearance dari editor/explorer; perubahan tampilan berlaku setelah Apply |
| F12 | Tutup panel dengan guard per dokumen |
| Alt+Shift+Left/Right atau Up/Down | Geser divider Columns terdekat dua sel / Rows satu sel |

Ctrl+C tanpa seleksi tidak keluar atau menyalin seluruh dokumen. Modal mengambil input field, bukan meneruskan shortcut editor ke dokumen belakang. Ctrl+H tetap Backspace karena byte terminal sama; Replace memakai Ctrl+R.

Appearance berisi draft tema, mode warna dan profil ikon; tampilan aktual baru berubah saat Apply. Tab/Shift+Tab atau Up/Down mengunjungi tiga row lalu Apply/Cancel; Left/Right mengubah nilai row, Space mengganti nilai atau mengaktifkan tombol, Enter/A menerapkan kecuali Enter pada Cancel. Mnemonic t/T, c/C dan i/I memilih row lalu next/previous; uppercase atau Shift bergerak ke pilihan sebelumnya. Mouse prev/next memakai aksi yang sama, dan klik Apply tidak bergantung pada tombol yang sebelumnya fokus.

Esc membuang draft; F1 kembali dengan draft/fokus yang sama. Minimum 40×10 berlaku pada renderer dan input; di bawahnya tidak ada Apply/cycle tersembunyi. Pilihan hanya untuk sesi berjalan: tidak menulis konfigurasi/snapshot atau mengubah dokumen, seleksi, viewport dan clipboard. F11/Alt+P tidak membuka Appearance di atas dialog lain; tidak ada perubahan shortcut F12 atau pemasangan font.

Helper memprioritaskan aksi inti, bukan seluruh shortcut split. F1 dan `--help` memakai katalog berkelompok yang sama. Bantuan memakai panah, PageUp/Down, Home/End dan wheel; page mengikuti jumlah baris visual yang benar-benar terlihat. Flow control, font dan protokol terminal masih memerlukan matriks validasi lebih luas.

## Siklus hidup panel dan dokumen

- Membuat split menampilkan dokumen aktif dengan view independen; teks tetap shared.
- Menutup panel yang memiliki beberapa tab memeriksa dokumen yang akan kehilangan view terakhir.
- Menutup view dokumen yang masih terlihat di panel lain tidak membuang dokumen tersebut.
- Penutupan view terakhir dokumen dirty: Save, Discard, atau Cancel.
- Save gagal atau dibatalkan berarti penutupan/exit dibatalkan.
- Quit pada sesi default menyimpan workspace/buffer terlebih dahulu; jika gagal, tetap di editor. Dialog quit pada launch terpisah memeriksa setiap dokumen dirty; jangan menafsirkan Discard satu dokumen sebagai Discard All.
- Panel terakhir tidak hilang menjadi layar buntu; usulan: ganti tab terakhir yang ditutup dengan dokumen kosong.

## Resize dan kondisi terbatas

- Pertahankan rasio split sejauh ukuran minimum memungkinkan.
- Jika layout tidak muat, sembunyikan sementara helper/explorer lalu tampilkan hanya panel aktif bila perlu; struktur panel dan buffer tidak dihapus.
- Jika area editor minimum tetap tidak muat, prioritaskan petunjuk `Ctrl+Q` atau `^Q` sebelum kalimat notice panjang agar jalur keluar tetap terbaca.
- Mengembalikan ukuran terminal memulihkan layout tanpa kehilangan state editing.
- Konfirmasi hanya aktif jika seluruh pesan dan tombol muat berdasarkan perhitungan yang sama pada renderer/controller; jika tidak, tampilkan Resize/Esc dan jangan daftarkan hit tombol atau menerima mnemonic tersembunyi.

## Error dan status

Pesan error menyebut operasi, target, dan tindakan yang bisa dilakukan, tanpa menyembunyikan kegagalan. Open/Save As gagal tetap menyimpan field, caret/selection dan pending Save/Close/Quit untuk retry; Esc membatalkannya. Bedakan “tersalin ke clipboard internal” dengan “tersalin ke clipboard sistem”; kegagalan System tidak menjanjikan paste dari internal. File unsupported tampil dengan alasan, bukan buffer kosong yang tampak berhasil dimuat.

Severity status dibawa secara eksplisit, tidak ditentukan dari prefix string. Pesan yang muat tidak dibatasi setengah baris demi path; error/warning mendahului path dan metadata cursor/format opsional. Elisi tengah mempertahankan awal aksi dan akhir penyebab pada batas grapheme. Warning highlight tetap tersimpan pada cache dokumen dan hanya menjadi note tambahan ketika ada ruang; ia tidak mengganti status foreground pada idle tick. Tinggi dock/helper dan area klik tidak berubah.

## Realisasi workspace dan batas visual

- Header/explorer global, split dua orientasi rekursif dan status/helper/dialog global tersedia. Explorer dan setiap editor memiliki frame penuh dengan judul/ikon; tab/gutter/body berada di dalamnya. Fokus memakai border accent/bold serta `●` atau `*` pada ASCII, bukan warna saja. Tab/close/overflow memakai view ID stabil.
- Dark/Light/Terminal, auto/truecolor/256/16/none dan Unicode/Nerd/ASCII tersedia melalui CLI. Kontras RGB/xterm-256 diuji; ANSI16/Terminal dikendalikan palet pengguna. Tanpa warna tetap memakai marker, reverse dan bold.
- Find/Replace literal case-sensitive dengan range hasil terseleksi, wrap, F6 tervalidasi dan F7 satu transaksi undo. Tidak ada regex, case folding atau marker semua hasil. Syntax Rust/Python/JSON dan konteks multiline dibuktikan; parser tidak berjalan dalam draw.
- Ctrl+N/O menargetkan panel terakhir; dokumen yang sama memakai satu registry/history lintas panel. Ctrl+W menutup view dan hanya meminta guard saat view terakhir dirty. F12 memeriksa seluruh dokumen final dalam panel; Ctrl+Q memeriksa dirty sekali per dokumen. Save gagal tidak meneruskan close/quit; dialog path mempertahankan continuation untuk retry, sementara Esc membatalkannya. Discard berlaku langsung per dokumen, bukan transaksi seluruh rangkaian.
- Ctrl+S/F4, konflik eksternal dan metadata save tetap memakai kebijakan keamanan. Field path/search/replacement memiliki caret grapheme, Shift selection, Ctrl+A/U, Backspace/Delete, routing clipboard backend terpilih dan klik/drag dengan window mengikuti caret. Ctrl+Z/Y modal tidak mencapai dokumen belakang. CLI mendukung path non-UTF-8.
- Mouse membuka file/folder, mengaktifkan/menutup tab di panel yang benar, me-resize explorer/split dan memilih teks. Border hanya memfokuskan panel; gutter tetap menavigasi baris. Wheel bekerja di seluruh kartu tanpa mengganti fokus; drag lintas panel tetap dimiliki view asal. Scroll keyboard/autoscroll serta edit menginvalidasi hit stale hingga frame baru tanpa membuang snapshot layout.
- Wheel horizontal kanan dibatasi oleh lebar sel baris yang terlihat plus ruang caret EOL, bukan menambah offset ke ruang kosong tanpa batas. Lebar dihitung saat aksi wheel, bukan dengan menyalin atau mengindeks seluruh dokumen per frame.
- Minimum tunggal compact 12×6; tiap panel minimum lebar max(12, digit gutter + 5), tinggi 4 baris termasuk border/tab/teks. Chrome global dipadatkan sebelum menyembunyikan panel. Split ditolak jika minimum tree baru tidak muat dengan chrome compact tanpa explorer. Resize dapat menyembunyikan explorer/panel lain sementara, tanpa mengubah ratio/tree/view. Modal umum memakai minimum dasar 20×6, search 40×10; Choices juga harus memenuhi tinggi/lebar seluruh pesan dan tombol.
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
- Explorer dan setiap editor/split mendapat frame penuh. Tab berada di dalam frame dan membawa ikon file. Border tidak menjadi posisi dokumen; gutter tetap memetakan navigasi baris. Resize tersedia melalui gap kolom atau border pemisah atas–bawah, bukan baris kosong tambahan.
- Header workspace dan frame status tersedia saat lebar ≥40, tinggi ≥18 dan seluruh tree masih muat setelah overhead 7 baris. Header 3 baris, status 3 baris dan helper biasa 1 baris langsung berdampingan dengan body tanpa gap kosong. Compact mengurangi chrome global dahulu; minimum panel 12×4 dan editor tunggal compact 12×6 tetap.
- Unicode default memakai ikon grafis, bukan badge `[F]`/`[D]`; Nerd tetap opt-in untuk font yang sesuai dan ASCII tetap tersedia. Warna ikon semantik tidak menimpa style seleksi.
- Geometry berada di modul `chrome`, dipakai renderer dan controller bersama agar split preflight, scroll, klik, drag dan resize tidak memakai offset lama.
- Pada batch ini format, 217 unit test, build dan skenario PTY lolos. Gap global saat itu memakai canvas solid Dark/Light; keputusan background tersebut digantikan oleh penyesuaian terminal glass di bawah. Glyph explorer Unicode dipilih agar lebar selnya konsisten pada library aplikasi dan parser terminal yang diperiksa. Desain ini tersedia untuk evaluasi pengguna, bukan pernyataan seluruh tahap kesiapan rilis selesai.

## Hasil penyesuaian helper, ikon dan browser folder

- Helper menjadi baris terpisah tanpa frame/label Keys, memakai kelompok `[tombol] Aksi` dengan separator titik; label normal/muted dan keycap tegas. Shortcut aktual editor tidak diganti menjadi huruf tunggal dari referensi.
- Background dasar header, explorer, editor, gutter, tab, gap dan status diseragamkan; hanya seleksi/fokus kontrol memakai penekanan semantik. Tidak ada strip background khusus baris cursor.
- Ikon Nerd memakai keluarga Material Design solid dan modifier bold; Unicode/ASCII juga bold tanpa mengganti font terminal pengguna. Hasil bentuk/weight bergantung font dan terminal. Profil Unicode/Nerd/ASCII tetap ada.
- Explorer menjadi browser folder dengan baris `..` di atas tree dan Backspace untuk naik. Enter membuka folder sebagai root browsing baru; Left/Right tetap collapse/expand tree, klik folder tetap toggle. Perubahan root tidak menutup dokumen atau mengubah working directory proses.
- Naik di filesystem root tidak melakukan apa-apa; kegagalan listing mempertahankan browser sebelumnya. Folder symlink tidak ditelusuri otomatis. Belum ada history Back/Forward atau operasi file manager.
- Folder `testfile` berisi tujuh contoh yang tidak dieksekusi aplikasi. YAML/YML, JS, HTML dan CSS tetap ter-highlight. Batch berikutnya menambahkan grammar TypeScript/TSX asli, termasuk JSX, sehingga kedua contoh tidak lagi plaintext atau alias parser JavaScript.

- PTY membuktikan parent lewat keyboard/helper/mouse, Enter, folder kosong, root `/`, refresh, wheel baris parent, no-follow saat directory berubah menjadi symlink, serta shared save/undo dengan CWD tetap. Empat profil tema/warna/ikon menjaga alignment, bold ikon, label helper non-bold dan modal isolation.
- Frame/cell dari aplikasi nyata diraster dengan font Nerd Mono sementara untuk inspeksi komposisi CSS/JS/YAML. Font/config sistem tidak diubah. Bukti unit/build dan batas dukungan berada di ROADMAP.md.

## Keputusan interaksi session restore

Pengguna memilih aktivasi otomatis pada `naf` tanpa path. Lima sesi TUI Linux/tmux membuktikan alur normal-exit/restore dan isolasi launch terpisah; detail serta batas di [ROADMAP.md](ROADMAP.md#hasil-session-restore-otomatis).

- Ctrl+Q pada sesi aktif menyimpan workspace beserta buffer Untitled/dirty, lalu keluar hanya setelah simpan sesi berhasil. File sumber tidak otomatis disimpan atau ditimpa. Gagal simpan menampilkan error dan tetap di editor dengan semua buffer utuh.
- Ctrl+W, close tab dan F12 tetap memakai Save/Discard/Cancel saat dokumen kehilangan view terakhir. Discard eksplisit menghapus buffer dari workspace dan snapshot berikutnya tidak menghidupkannya kembali.
- `--no-session` atau launch dengan path file/folder mempertahankan alur lama: tidak ada snapshot, dan Ctrl+Q memakai guard Save/Discard/Cancel. `--no-config` hanya mengabaikan preferensi, bukan sesi.
- Startup memulihkan tab/panel aktif, urutan tab, split/ratio, cursor/selection/viewport dan root/visibility/fokus/lebar explorer. State yang pulih tetap dapat dipadatkan saat terminal kecil; tidak menghapus panel tersembunyi. Undo/redo dimulai ulang setelah restore.
- Snapshot memuat isi buffer, dapat berisi rahasia, dan tidak dienkripsi. Status startup membedakan sesi aktif/dipulihkan dari launch terpisah. Ini penyimpanan saat keluar normal, bukan autosave/crash recovery; signal, panic dan proses mati tidak menyimpan snapshot baru.


## Penyesuaian untuk terminal glass

Atas permintaan pengguna, background solid Dark/Light diganti dengan default terminal tanpa flag baru. Highlight tetap dibatasi pada state yang perlu dikenali; active tab tidak lagi bergantung pada bold/marker saja. Spacing tab-ke-teks menggunakan geometry bersama, termasuk pemadatan pada terminal minimum dan hit testing yang tidak menganggap baris kosong sebagai teks.

Perubahan ini dibuktikan melalui aplikasi release pada tiga profil terminal lokal; [metode dan batas verifikasi](ROADMAP.md#hasil-background-terminal-dan-tab-aksesibel). Transparansi compositor dan metrik Nerd Font pada client fisik tetap dikendalikan terminal, bukan aplikasi.

