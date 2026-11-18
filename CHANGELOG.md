# Changelog

## 0.1.1 — 2026-11-19

- Tambahkan bootstrap Bash untuk mengunduh latest Release, memverifikasi SHA-256 dan menyiapkan payload privat sebelum menjalankan installer bundle; opsi serta status helper diteruskan dan konfigurasi lama dipertahankan.
- Dokumentasikan instalasi curl dengan `pipefail`, upgrade dan alternatif bundle offline; QA bootstrap dan publikasi endpoint belum diklaim.
- Tambahkan gate audit paket di CI sebelum upload artifact: checksum sidecar ketat, whitelist enam file reguler, VERSION/ELF, helper serta notice identik checkout, metadata owner ternormalisasi, dan scan kategori credential/path build tanpa mencetak nilai.
- Normalisasi owner/group archive menjadi numerik `0` tanpa `uname`/`gname` agar metadata build lokal tidak ikut terdistribusi; format dan isi paket tidak berubah.
- Netralkan path fixture dan metadata penulis sebelum publikasi; history serta tag disanitasi dengan backup privat.

- Perbaiki resolusi commit tag pada workflow draft Release: parameter ref memakai nama tag langsung, tanpa prefix `tags/` yang ditolak GitHub dengan HTTP 422.
- Metadata paket, lockfile dan panduan instalasi memakai 0.1.1; perilaku editor tidak berubah.

## 0.1.0 — 2026-11-18

### Appearance persisten dan shortcut Alt+P

- Lepas binding F11 agar tidak bentrok dengan fullscreen/maximize terminal atau window manager; Alt+P dan chip Style membuka Appearance dari editor/explorer.
- Apply menyimpan tema, warna dan ikon lewat writer konfigurasi yang sama dengan CLI sebelum menerapkannya. Target mengikuti konfigurasi peluncuran; clipboard tersimpan tidak ditimpa override runtime. `--no-config` tetap run-only dengan pemberitahuan.
- Gagal simpan mempertahankan draft/fokus dan tampilan lama, menampilkan error tersanitasi pada dialog minimum 40×10 dan mendukung retry/F1 tanpa kehilangan draft. Cancel tidak menulis konfigurasi.
- Delapan regresi unik, format/check, build release dan lima skenario PTY tmux lulus melalui DeepSeek atmo, termasuk restart, target config kustom dan save failure. [Bukti dan batas](docs/ROADMAP.md#hasil-persistensi-appearance); binary terpasang pengguna dan CI hosted belum diperbarui.

### Installer bundle dan draft rilis Linux

- Tambahkan installer user-local tanpa download/sudo otomatis, opsi prefix dan empat preferensi, serta konfigurasi baru melalui writer aplikasi. Konfigurasi lama selalu dipertahankan; pembaruan binary berbeda memerlukan `--replace`.
- Satukan packaging lokal/CI dengan binary, installer, versi SemVer, template, notice dan sidecar SHA-256; escape output path. Smoke CI memakai prefix berspasi dan CWD di luar bundle serta memeriksa replay config berkomentar.
- Tambahkan CI reusable dan workflow draft Release manual pada tag yang cocok dengan versi binary; resolusi commit tag diperiksa kembali sebelum membuat draft. Hanya job draft memiliki izin write; tidak membuat tag, memublikasikan otomatis atau mengubah visibilitas/lisensi repo.
- QA normal Terra Codex lulus pada binary release yang sudah tersedia: syntax shell, parsing YAML, packaging/checksum, body smoke CI asli, instalasi default dan `--skip-config`. Parsing bukan validasi schema GitHub; workflow hosted/rilis belum dijalankan. Bukti serta batas ada di [roadmap distribusi](docs/ROADMAP.md#distribusi--persiapan-installer-dan-draft-rilis).
- QA integritas/negative dan recheck DeepSeek atmo lulus pada jalur teruji; tutup temuan validasi versi, commit tag dan output path. Parent directory tetap harus dipercaya, alias direktori tetap diikuti dan pemeriksaan tag bukan lock atomik. Root hanya diuji lewat fakeroot; workflow/API hosted belum dijalankan.

### Perbaikan CI

- Rapikan format assertion caret pada tes field sesuai rustfmt agar gate formatting CI dapat dilalui; tidak mengubah perilaku aplikasi.

### Ergonomi field dan navigasi Explorer

- Tambahkan undo/redo delta lokal pada field, Ctrl+Backspace/Delete, serta drag autoscroll dengan kepemilikan capture dan guard frame stale.
- Tambahkan type-ahead jump pada row Explorer ter-load, timeout dan cycling; tampilkan issue entri serta kartu error root dengan Retry/Up.
- Terima tombol xterm modifyOtherKeys tanpa mengubah byte legacy atau menegosiasikan mode terminal baru. Ctrl+Backspace/Ctrl+Shift+Z tetap bergantung pada encoding terminal.
- Sebanyak 24 kasus target berbeda serta smoke TUI normal/input lulus setelah perbaikan affordance parent tak diketahui. QA normal memakai Terra Codex; input/negative dan error permission memakai DeepSeek atmo. [Bukti, hash dan batas](docs/ROADMAP.md#hasil-ergonomi-field-dan-navigasi-explorer).

### Switcher tampilan runtime

- Tambahkan Appearance lewat F11/Alt+P untuk tema, mode warna dan profil ikon, dengan kontrol keyboard/mouse serta bantuan F1 yang memulihkan draft/fokus.
- Pisahkan draft dari preferensi aktif: Apply/Enter menerapkan, Esc/Cancel membatalkan. Perubahan hanya berlaku untuk run ini; tidak menulis konfigurasi/sesi atau mengganti clipboard/font.
- Pertahankan dokumen, undo, cursor, seleksi dan viewport ketika menu ditutup; invalidasi target klik lama. Pada ukuran di bawah 40×10, hanya Esc/F1 yang aktif.
- Bangun ulang palet hanya ketika tema/warna berubah, tanpa retokenisasi syntax. Background utama tetap mengikuti terminal; Nerd tetap opt-in dan F12 tetap menutup pane.
- Empat tes Appearance dan smoke TUI terarah melalui `openai-codex/gpt-6.1-terra` lulus; fixture/config/binary tidak berubah dan aplikasi exit 0. [Bukti, hash dan batas](docs/ROADMAP.md#hasil-switcher-tampilan-runtime).

### Indikator overflow field

- Tambahkan indikator pasif `‹`/`›` pada path, Find dan Replace, dengan fallback `<`/`>` pada ASCII; sisi tanpa teks tersembunyi tetap kosong.
- Sisihkan gutter dari `FieldHit` dan perhitungan caret agar indikator tidak memilih teks/mengganti fokus. Pada satu/dua sel nilai, pertahankan caret tanpa gutter.
- Pertahankan source byte/grapheme, seleksi, clipping dan sanitasi lama; tanpa salinan field atau pemindaian teks tambahan. Bukan scrollbar interaktif atau autoscroll.
- Tiga kasus terarah serta TUI nyata lulus setelah migrasi assertion caret: hit setelah resize, seleksi path, clipboard penuh dan return F1 dibuktikan lewat output fixture identik byte-per-byte. Penutupan memakai `openai-codex/gpt-6.1-terra`; [hash, metode dan batas](docs/ROADMAP.md#hasil-indikator-overflow-field).

### Scrollbar Explorer dan bantuan F1

- Tambahkan rail vertikal di baris tree dan katalog bantuan, terpisah dari parent row, divider, border dan footer. Track yang terlalu kecil tidak diterbitkan.
- Satukan capture/page/drag editor, Explorer dan Help tanpa mengubah pilihan file, fokus atau field modal; key dan perubahan konteks membatalkan capture lama.
- Hit row mengecualikan rail; wrapping, page dan limit F1 memakai lebar katalog yang sama. Memoisasi jumlah row per lebar katalog statis menghindari penghitungan ulang per frame tanpa salinan katalog.
- Pertahankan background terminal dan fallback ASCII/Unicode/Nerd; katalog F1/CLI menjelaskan gesture scrollbar. Field overflow serta theme/keymap runtime tetap di luar cakupan.
- Verifikasi DeepSeek: 130 kasus tes berbeda pada empat filter terarah serta TUI nyata lulus; [hash, metode dan batas](docs/ROADMAP.md#hasil-scrollbar-explorer-dan-f1). Bukan suite seluruh target atau matriks font/mouse fisik.

### Scrollbar editor per view

- Tambahkan track dan thumb vertikal/horizontal di dalam frame, dengan klik page serta drag terikat pane asal tanpa mengubah fokus, cursor, seleksi atau history.
- Pertahankan background terminal, bentuk ASCII/tipis-tebal dan minimum teks; rail tidak bertabrakan dengan border resize split. Wheel/autoscroll memakai batas vertikal viewport yang sama.
- Ukur lebar render baris terlihat secara bertahap dan simpan cache per view/revisi; track horizontal menandai hasil pending tanpa thumb palsu. Cache tidak dipersist dan tidak menyalin dokumen. Explorer/help/field di luar cakupan.
- Perbaiki rail satu sel/thumb nol pada gutter lebar, endpoint thumb terkuantisasi dan clamp pane nonaktif setelah resize. Pisahkan redraw metadata dari invalidasi syntax; 88 tes recheck serta TUI DeepSeek pada build akhir lulus. [Bukti, hash dan batas](docs/ROADMAP.md#hasil-scrollbar-editor).

### Indent dan dedent blok

- Tab dengan seleksi multibaris menambah empat spasi per baris tanpa mengganti isi seleksi; endpoint awal baris berikutnya dikecualikan. Tab biasa/seleksi satu baris tetap memakai tab stop empat kolom.
- Shift+Tab mengurangi satu tab awal atau hingga empat spasi pada baris aktif/terpilih, menjaga boundary grapheme dan arah seleksi. No-op tidak membuat dirty/history atau menghapus redo.
- Satu perintah blok menjadi satu undo bersama; prefix disimpan ringkas tanpa salinan seluruh blok, dan cursor/anchor/viewport shared view dipetakan per perubahan prefix.
- Format, 113 tes terarah, build release dan 23 skenario TUI DeepSeek lolos; [bukti serta batas](docs/ROADMAP.md#hasil-indent-dan-dedent-blok). Tidak menambahkan scrollbar, autoindent, pengaturan tab width, atau menjalankan ulang audit/soak tahap 4.

### P1 aksesibilitas dan Find/Replace

- Prioritaskan Help/All, Quit, Files dan Pane pada helper 80 kolom; panah overflow tab yang nonaktif tidak menerbitkan hit klik.
- Tambahkan fokus tombol konfirmasi dengan Tab/Shift+Tab/panah dan Enter/Space, default Cancel, serta target path/Untitled. F1 mempertahankan fokus dan guard ukuran mencakup konteks target.
- Gunakan Alt+Shift+Left/Right untuk resize explorer saat fokus, berbasis lebar yang terlihat; shortcut editor tetap menggeser split.
- Prefill Find/Replace dari seleksi satu baris yang muat; tampilkan penghitung, total berbatas `100+`, posisi `>100` serta penanda wrapped. Cache tidak dipindai per-frame dan diinvalidasi saat reload.
- Selaraskan navigasi Find overlapping dua arah dengan hitungan; replacement non-overlap dan shared undo tetap dipertahankan.
- Perbaiki override warna CLI/config yang masih dikalahkan `NO_COLOR` oleh Crossterm; tema menjadi satu pemilik keputusan. Auto/None tetap tanpa warna, tanpa mutasi environment pengguna.
- Tutup tahap 4 untuk matriks Linux teruji sesuai keputusan pengguna: P1 unit/runtime, VTE dengan Nerd Font Mono, client tmux, SSH loopback dan soak; [bukti, urutan build dan batas](docs/ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian). Publish/installer serta client fisik/non-loopback tidak diklaim selesai.

### Frame terpisah tanpa indikator horizontal

- Kembalikan dua frame atas–bawah tanpa baris kosong tambahan; jangan gabungkan border. Jarak kiri–kanan tetap satu kolom.
- Hapus titik combining dan fallback underscore horizontal setelah laporan glitch client; border/corner tetap utuh. Grip kolom dan resize mouse/keyboard tetap tersedia tanpa mengubah minimum, gap atau hit resize.
- Hapus junction dan clipping judul khusus shared border; judul kembali pada frame masing-masing.
- Format, satu tes regresi (78 frame), build release dan dua smoke Nerd/ASCII lolos, termasuk dua panel minimum 12×11; [bukti serta batas](docs/ROADMAP.md#hasil-penghapusan-indikator-horizontal). Tidak menjalankan security test.

### Jarak split atas–bawah

- Hilangkan baris kosong antar-panel atas–bawah; pertahankan gap kiri–kanan satu kolom dan jarak tab-ke-teks.
- Pertahankan frame dan resize lewat border pemisah; selaraskan minimum/preflight serta offset drag/keyboard.
- Verifikasi 17 target tes terarah dan smoke release Nerd/ASCII, termasuk dua panel minimum pada 12×11; [urutan pemeriksaan dan batas](docs/ROADMAP.md#hasil-perapatan-jarak-split-bertingkat). Tidak menjalankan security test.

### Background terminal dan tab aktif

- Ikuti background default terminal pada semua tema agar canvas tidak menutup efek glass/transparan; foreground tema tetap tersedia tanpa flag baru.
- Beri tab aktif background seleksi dengan foreground terbaca untuk label, ikon, dirty/read-only dan close. Marker/bold serta fallback reverse tetap tersedia.
- Tambahkan satu baris kosong antara tab dan isi editor; padatkan pada panel minimum agar 12×6 tetap dapat dipakai dan hit mouse tetap sesuai.
- Gunakan pasangan warna seleksi untuk prefix field fokus; hapus accessor gaya yang tidak lagi dipakai.
- Format, 14 tes terarah, build release dan tiga sesi TUI lokal lolos; [bukti serta batas](docs/ROADMAP.md#hasil-background-terminal-dan-tab-aksesibel). Tidak menjalankan security test.

### Perbaikan bug P0 audit AGY

- Perbaiki warna eksplisit border/indikator explorer/canvas compact dan kontras marker explorer terpilih tanpa mengganti palet.
- Pertahankan input path, caret, seleksi, dan kelanjutan Close/Quit ketika Open/Save As gagal; retry tetap tersedia setelah kegagalan overwrite.
- Nonaktifkan pilihan destruktif jika seluruh pesan dan tombol tidak muat; pertahankan petunjuk keluar pada terminal kecil. Perjelas bahwa Discard langsung berlaku per dokumen.
- Pisahkan level status dan catatan highlight persisten; warning highlight tidak lagi menimpa hasil operasi pengguna.
- Beri petunjuk recovery `Ctrl+G` untuk urutan input terminal yang belum selesai, tanpa mengubah payload kontrol menjadi shortcut.
- Hilangkan janji fallback clipboard internal pada kegagalan copy System; batasi scroll horizontal pada isi viewport.
- Build debug/release, suite oleh ATMO, dan runtime TUI diverifikasi; [bukti serta batasnya](docs/ROADMAP.md#hasil-perbaikan-bug-p0-audit-agy). Usulan P1/P2 belum diimplementasikan.

### Validasi workflow build release

- Build release terbaru dan CLI version sukses. Tiga sesi pada backend terminal Orca langsung, tanpa tmux, membuktikan edit Unicode/shared undo/Replace All, mouse CJK, clipboard internal/Save As serta restore Untitled/dirty dengan tiga panel.
- Semua sesi keluar 0 dan memulihkan termios; terminal dan fixture terisolasi dibersihkan tanpa mengambil fokus atau memakai file pengguna. Tidak mengubah kode produksi; rincian binary dan metode di [ROADMAP.md](docs/ROADMAP.md#hasil-validasi-workflow-release-pada-terminal-orca).
- Ini bukti screen backend, bukan validasi GUI/font fisik atau input keyboard/mouse client. Tidak menjalankan security test, suite luas, packaging, GitHub Actions, publish atau installer; kesiapan rilis keseluruhan tetap terbuka.

### Visibilitas explorer saat startup

- `naf` tanpa path selalu menampilkan explorer, termasuk saat sesi sebelumnya menyembunyikannya; buffer, tab/split, root dan lebar tersimpan tetap dipulihkan. Argumen folder menampilkan explorer, argumen file existing/baru menyembunyikannya.
- Gunakan Ctrl+B yang sudah tersedia untuk hide/unhide tanpa membuang state; drag resize dan layout compact tetap berlaku. Help CLI dan dokumentasi menjelaskan aturan startup baru; tidak menambah flag atau mengubah format snapshot.
- Format, satu tes navigasi/fokus explorer dan build dev lolos. Tujuh sesi TUI tmux membuktikan startup/restore, toggle, mouse/edit/save/undo, resize dan pemulihan lebar; semua keluar 0 dan memulihkan termios/alternate screen. Tidak menjalankan security test atau suite luas.

### Session restore otomatis

- `naf` tanpa path memulihkan workspace dari exit normal; Ctrl+Q menyimpan sesi beserta Untitled/dirty tanpa menulis file sumber. `--no-session` serta launch file/folder tetap terpisah dan memakai dirty guard lama.
- Simpan satu teks per dokumen shared beserta baseline asli, format/read-only/dirty, tab/panel aktif, split/ratio, cursor/selection/viewport dan root/visibility/fokus/lebar explorer. Undo/redo dimulai ulang, bukan dirty state.
- Tambah storage privat XDG_STATE_HOME/HOME, lock eksklusif nonblocking, metadata bertipe berbatas, payload Rope streaming, checksum BLAKE3 dan atomic replacement. Gagal simpan tetap di editor; snapshot rusak tidak diubah menjadi sesi kosong. Snapshot bukan enkripsi, autosave atau crash recovery.
- Tambah serde langsung dan serde_json; lockfile menambahkan serde_json/memchr dengan metadata lisensi/MSRV diverifikasi. Audit dependency lama tidak dianggap mencakup lockfile baru.
- Satu regresi dirty checkpoint dan empat tes CLI/config/theme lolos; build dev serta lima sesi TUI normal-exit/restore, shared save, BOM/CRLF, Discard, launch terpisah dan resize dibuktikan. Security test terarah dikerjakan model `atmo/alicn/deepseek-v4.1-flash-1017`, tanpa temuan kerentanan sesi terkonfirmasi; pesan saran `--no-session` ganda diperbaiki dan diamati ulang oleh model yang sama. Rincian/batas ada di [ROADMAP.md](docs/ROADMAP.md#hasil-session-restore-otomatis) dan [SECURITY.md](docs/SECURITY.md#hasil-verifikasi-model-terpisah).

### Navigasi kata panjang dan validasi editor tmux

- Percepat Ctrl+Left/Right dan double-click pada run kata/spasi panjang melalui iterator char Rope berurutan. Kelas tetap mengikuti karakter pertama setiap grapheme; combining mark, Prepend, emoji dan newline tidak dipotong menjadi batas palsu.
- Pindahkan navigasi kata directional ke `Document`; hapus pemindaian kedua arah yang tidak diperlukan dan loop whitespace per-grapheme dari controller. Tidak menambah salinan dokumen, cache atau dependency.
- 19 tes dokumen lolos, termasuk tiga regresi Unicode/whitespace/chunk; build dev sukses. Empat profil editor di tmux membuktikan seleksi keyboard, double-click, shared undo/redo/save, close panel, resize dan restorasi terminal dengan bytes fixture kembali tepat.
- Pengukuran sebelum/sesudah beserta metode dan batas ada di [ROADMAP.md](docs/ROADMAP.md#hasil-navigasi-kata-panjang-dan-validasi-editor-tmux). Tidak menjalankan security test, menambah session restore, installer atau janji dukungan terminal fisik.

### Audit keamanan dependency dan integritas persist

- Audit `cargo-audit 0.23.0` (binary rilis RustSec dengan sha256 terverifikasi) atas 119 dependency Linux/x86_64: nol vulnerability. Tiga paket unmaintained (`paste`, `bincode`, `yaml-rust`) dan dua advisory unsound `lru 0.12.5` lewat ratatui dicatat bersama jalur dependency dan batas reachability di [SECURITY.md](docs/SECURITY.md#hasil-audit-dependency-2026-11-14).
- Perbaiki substitusi nama temporary pada save: nama divalidasi terhadap dev/inode/link count descriptor penulis tepat sebelum replace, dan direktori tujuan diverifikasi menunjuk inode yang sama sesudah replace. Ketidakcocokan menghasilkan `FileError::Substituted` sehingga buffer tetap dirty, bukan checkpoint palsu.
- Perbaiki prefix SS3 terpotong (`\x1bO`) yang menelan ESC pembuka bracketed paste; ESC baru kini diparse dari byte pertamanya, jadi payload paste tidak lagi menjadi shortcut (termasuk Ctrl+A/Ctrl+S). Double escape setelah prefix tetap menghasilkan satu Esc. Sesi TUI nyata membuktikan perbedaan: build tanpa perbaikan menyimpan `REPLACED` tanpa aksi pengguna, build dengan perbaikan menyimpan payload literal `\x01REPLACED\x13` sebagai data sampai Ctrl+S eksplisit.
- Tambah tiga regresi (substitusi nama temporary, binding inode pada save sukses, prefix SS3 sebelum paste dan double escape). `cargo fmt --check` dan **274 tes** lolos. Celah check→rename, reachability `lru`, varian ACL/security label, dan fault injection panic tetap terbuka.

### Executable naf

- Ganti satu-satunya target executable menjadi `naf`; nama aplikasi/package Notefart dan namespace konfigurasi `notefart` tetap. Tidak menambahkan alias binary lama.
- `naf` membuka editor kosong dan `naf test.txt` langsung membuka file; path baru dibuat saat Save. Help/version/error prefix, workflow build/artifact dan panduan aktif diselaraskan.
- Format, tiga tes CLI, build dev/release, tiga sesi TUI dan packaging lokal diverifikasi. Binary lama di debug/release/dist dibersihkan; installer/PATH pengguna dan remote GitHub tidak diubah.

### CI Linux dan regresi lengkap

- Tambah GitHub Actions untuk push/PR/manual: format, seluruh target tes, build release, smoke CLI dan artifact Linux x86_64 dengan template serta notice. Toolchain Rust 1.99.1 dipin; Action resmi memakai full SHA, izin read-only dan checkout tanpa credential persisten.
- Artifact berupa tar dengan executable bit, retensi 7 hari, tanpa auto-publish Release atau installer. Ignore config pribadi/.env tanpa mengabaikan template.
- Pemeriksaan lokal: format lolos, **271 tes lolos**, release build dan smoke CLI sukses; actionlint lolos serta packaging/isi/mode binary diverifikasi. Run/upload GitHub belum dilakukan; remote dan repository private belum dibuat/disetel.

### Geometri sparse untuk baris panjang

- Tambah cache geometri turunan per dokumen: maksimum 8 baris panjang, checkpoint sekitar setiap 1024 char dan frontier terakhir. Pencarian kolom/posisi serta render horizontal memulai dari batas grapheme terdekat, bukan selalu awal baris.
- Edit/undo/redo mempertahankan prefix aman dan menginvalidasi suffix; cache baris sesudah edit dibuang dan clone tidak menyalinnya. Fase tab, koordinat char/byte/cell, seleksi dan escape tetap berlaku.
- Perbaiki pairing regional-indicator lintas chunk yang ditemukan saat menguji resumption. Batas sel streaming dibandingkan dengan segmentasi string utuh, bukan mengubah flag menjadi karakter terpisah.
- 16 tes dokumen dan 31 renderer lolos, build serta TUI membuktikan End/edit/undo/mouse, shared save/undo dan resize. Perbandingan sebelum/sesudah beserta batas lookup dingin ada di [ROADMAP.md](docs/ROADMAP.md#hasil-geometri-sparse-untuk-baris-panjang). Tidak menambah soft wrap, session atau installer.

### Perbaikan warna Markdown

- Scope markup kini dipetakan ke token heading, emphasis, code, link, quote dan marker; sebelumnya hanya kategori bahasa pemrograman yang mendapat warna sehingga Markdown tampak plaintext.
- Gunakan palet tema yang sudah ada tanpa mengganti grammar/parser, cache, slice atau renderer. Paragraf/list body tetap biasa, scope nested tetap berlaku dan seleksi tetap mengalahkan warna syntax.
- 19 tes highlighter, 7 tema dan 4 syntax lolos; build serta lima sesi TUI membuktikan warna `AGENTS.md`, scroll balik, edit/undo code fence, seleksi, Dark/truecolor, Light/256, Terminal/16 dan mode none. Batas raw code/Setext dicatat di DESIGN; bukan preview Markdown.

### Optimasi lookahead tabel Markdown

- Tambah necessary-pipe lookahead saat membangun dump grammar: paragraf tanpa pipe tidak menjalankan balanced-cell table lookahead mahal. Aturan/capture/context asli dipertahankan; build menolak perubahan grammar yang perlu peninjauan.
- CPU catch-up EOF pada fixture lokal turun 0,80→0,56 detik; benchmark engine prefix sekitar 24% lebih singkat. Nano masih lebih ringan; tidak ada klaim startup/input/RAM lebih cepat atau batas keras CPU. Metode dan keterbatasan di [ROADMAP.md](docs/ROADMAP.md#hasil-optimasi-lookahead-tabel-markdown).
- Empat tes syntax dan build lolos; TUI membuktikan warna comment, invalidasi setelah edit delimiter, undo, page/wheel dan restorasi terminal. Tidak mengubah engine, long-line policy, session atau installer.

### Status panjang dan pengukuran dokumen besar

- Copy/Cut/Paste failed kini mendapat prioritas/style error. Status yang muat tidak dibatasi setengah baris demi path; error/warning dapat mengalahkan metadata opsional. Pesan berlebih memakai elisi tengah grapheme-safe yang mempertahankan awal aksi dan akhir penyebab, tanpa menambah tinggi dock atau buffer String per frame.
- Perhitungan metadata kolom dilewati jika pesan kritikal pasti mengambil seluruh baris. Helper, hit target, format dokumen dan batas load/save tetap.
- 29 tes renderer, build tanpa warning dan TUI nyata memverifikasi status/warna/profil ikon, clipboard unavailable, warning, Help serta resize. Pengukuran enam bentuk dokumen pada binary akhir beserta RSS/CPU dicatat di [ROADMAP.md](docs/ROADMAP.md#hasil-status-panjang-dan-profil-dokumen-besar).
- Pengukuran menemukan baris tunggal ekstrem masih lambat dan cold jump Markdown masih memakai CPU background. Batch ini tidak mengubah engine dokumen/highlighter atau mengklaim semua file hingga batas ukuran responsif. Session restore tetap backlog; installer ditunda sampai publish.

### Clipboard sistem dan preferensi pengguna

- Tambah clipboard Auto/Internal/System: Wayland melalui wl-clipboard, X11 melalui xclip pada CLIPBOARD; Auto SSH/tanpa helper memakai internal dengan alasan terlihat. Copy/paste editor dan Path/Find/Replace mengikuti fokus; tidak ada akses clipboard saat render/idle atau hanya karena seleksi.
- Copy Wayland memakai MIME UTF-8 eksplisit. Helper tetap/argv terpisah tanpa shell atau output terminal, pipe nonblocking, deadline 2 detik dan terminasi grup proses pada kegagalan. Paste eksternal maksimum 1 MiB UTF-8, ditolak utuh; kosong/gagal tidak menghapus seleksi atau memakai data internal lama. Cut gagal mempertahankan sumber.
- Tambah konfigurasi TOML pengguna untuk theme/color/icons/clipboard dengan prioritas bawaan < file < CLI. `--config`, `--no-config`, `--save-config` dan `--clipboard` tersedia. Simpan preferensi eksplisit bersifat atomik/private dan keluar tanpa TUI; startup biasa tidak menulisnya. Nerd Font dapat dipilih sekali tanpa mengulang flag pada launch berikutnya.
- File konfigurasi maksimum 64 KiB, unknown key/type/value ditolak sebelum raw mode, tanpa project config/eksekusi. Help/version/probe independen; target save link/non-regular ditolak.
- Build tanpa warning, tes terarah dan 11 sesi PTY Linux dibuktikan, termasuk X11+Wayland nyata terisolasi, shared undo, dialog, helper timeout dan konfigurasi rusak. Detail/batas di [ROADMAP.md](docs/ROADMAP.md). Tidak menginstal helper atau mengubah konfigurasi/font sistem; session restore tetap backlog.

### Performa Markdown, TypeScript/TSX dan gap layout

- Backend highlight diganti dari regex-fancy ke Oniguruma native. Token viewport yang keluar layar dipindah tanpa clone span ke FIFO recent maksimal 512 baris/2 MiB; edit/path/language/barrier/close menginvalidasi cache terkait dan gap besar tidak ditokenisasi.
- Parser memakai budget baris plus slice kooperatif 4 ms; frame pertama dilukis sebelum cold highlight dan tidak ada beberapa slice berulang sebelum frame/input yang sama. Idle berhenti mengerjakan parser setelah pending selesai; satu regex tidak dipotong paksa.
- Grammar TypeScript/TSX asli berlisensi, termasuk type/generic/template dan JSX, dibundel dari konversi bat. Build menghasilkan dump embedded uncompressed; SyntaxSet/regex data dibagi immutable lewat LazyLock. Runtime tidak memuat YAML grammar/proyek/jaringan.
- `cargo run` kini dev opt-level 1 dengan debug info/assertions; profil test tetap 0. Dua gap vertikal header–body–dock dihapus, sehingga overhead normal 9→7 baris tanpa menghapus frame/helper atau mengubah minimum compact.
- Benchmark input tetap serta CPU scroll, rerun tes cache/grammar/chrome, build dan delapan sesi PTY dibuktikan; angka dan urutan pemeriksaan di [ROADMAP.md](docs/ROADMAP.md). Validasi mencakup shared edit/save/undo, long Markdown jump, TS/TSX, mouse hit dan restore terminal. Native build memerlukan compiler C; notice disertakan.

### Helper berkelompok, ikon solid dan browser folder

- Helper menjadi baris biasa `[tombol] Aksi · …` di bawah frame status; label tidak seluruhnya bold/berwarna. Grup lengkap tetap clickable, separator tidak menjadi aksi dan shortcut editing tidak diganti.
- Ikon Nerd memakai Material Design solid terverifikasi dan modifier bold; Unicode/ASCII tetap tersedia. Background header/explorer/tab/editor/gutter/current line/gap/status/dialog seragam di `canvas`; token surface/raised dihapus. Seleksi/fokus semantik tetap dibedakan.
- Explorer: Enter/helper Open masuk folder, Backspace/helper Up/klik `..` naik; Left/Right dan klik folder tetap tree expand/collapse. Header/root sinkron setelah listing berhasil, tanpa menutup dokumen atau mengubah CWD. Root `/` no-op; listing gagal mempertahankan browser; directory-symlink swap ditolak dengan NOFOLLOW.
- Tambah tujuh contoh di `testfile`: YAML/YML/JS/HTML/CSS ter-highlight; TS/TSX masih plaintext karena bundled grammar belum tersedia. Tidak ada frontend/dependency baru yang dijalankan.
- Format/build, jalur unit lintas modul dan rerun 33 tes Explorer lolos; PTY membuktikan helper/mouse/parent/empty/root/refresh/permission recovery/race/shared save/undo/minimum/restore pada empat profil. Retry menyinkronkan root/header canonical setelah pulih. Detail urutan unit di ROADMAP.md. Font/config pengguna tidak diubah; history Back/Forward belum tersedia.

### Redesign visual bergaya superfile

- Explorer dan setiap editor/split kini memakai frame penuh membulat, judul/ikon terintegrasi dan penanda fokus accent/bold plus marker non-color. Tab menampilkan ikon kategori file; Unicode default memakai glyph grafis, Nerd opt-in dan ASCII tetap tersedia.
- Header workspace serta dock status/helper berbingkai saat ruang cukup. Gap satu sel memisahkan kartu dan tetap dapat di-drag; gap global memakai canvas tema eksplisit. Chrome compact dipilih sebelum mengorbankan panel yang masih muat.
- Geometry/minimum dipusatkan di `chrome` untuk renderer/controller/workspace. Minimum panel 12×4, editor tunggal compact 12×6; preflight split menghitung ruang compact maksimum.
- Border hanya memfokuskan panel; navigasi gutter dipertahankan. Wheel bekerja di seluruh kartu tanpa mencuri fokus. Paging, tab close dan drag memakai inner/outer geometry baru.
- Format, 217 unit test, build dan PTY nested frame/mouse/paging/tab/compact/collapse/minimum lolos. Empat profil tema/warna/ikon diperiksa; raster Nerd Mono dari grid PTY tersedia untuk inspeksi, bukan klaim validasi emulator/font fisik.

### Pemolesan field, bantuan dan chrome

- Marker fokus panel/explorer dipadatkan; footer memprioritaskan aksi inti. Status memakai metadata path/format/dirty terpisah, dengan prioritas error dan buffer yang dipakai ulang.
- Field path/Find/Replace mendukung caret/seleksi grapheme, navigasi tengah teks, Delete, clipboard internal dan klik/drag. Batas 4096 byte memperhitungkan normalisasi/replacement; penolakan tidak membuang isi/seleksi.
- Bantuan F1 berbagi katalog dengan `--help`, mendukung scroll/page/wheel/resize, dan mengembalikan modal/field serta pending Save/Close/Quit. Tombol yang terlihat memakai hit geometry dan aksi statis, bukan parsing nama file.
- Regresi ditutup untuk caret sesudah penggabungan grapheme, render Unicode tiga sel tanpa byte/control korup, gap kata help dan scroll lama sesudah resize.
- Format, 204 unit test, build dan alur PTY field/mouse/help/shared-search/save/quit lolos. Empat profil tema/warna/ikon diperiksa; font fisik/Nerd Font dan desain visual final belum diklaim.

### Hybrid panel + tab dan shared document

- F8/F9 split dua orientasi rekursif, F10/Shift+F10 pindah panel, F12 close panel. Tab tetap lokal per panel; Alt+Shift+Arrow dan drag mouse me-resize divider sesuai minimum subtree.
- Registry memiliki satu Document/history/path/baseline per identitas file. View memiliki cursor/selection/scroll independen, termasuk ketika hidden. Edit, undo/redo dan Replace All memetakan ulang posisi seluruh view melalui TextChange.
- Open/explorer menargetkan panel terakhir; alias lintas panel membuat view shared, bukan buffer terpisah. Save/Save As/dirty/read-only berlaku bersama; Save As mengecualikan dokumen sendiri dari collision. Reload memperbarui semua view.
- Close satu shared view tidak meminta discard; guard berlaku pada final view. Close panel memeriksa seluruh dokumen final, quit dirty sekali per dokumen. Save gagal/cancel menghentikan continuation tanpa kehilangan shared survivor.
- Layar kecil menyembunyikan explorer/panel sementara tanpa menghapus tree/ratio; split baru ditolak bila minimum tidak muat. Mouse tab/body/close, wheel tanpa focus steal dan drag asal-pane memakai hit ID stabil; scroll/resize menginvalidasi frame stale.
- Highlight cache per dokumen menggabungkan hanya viewport terlihat, bukan gap besar antarpanel. Budget/checkpoint/long-line policy tetap berlaku. UI memakai reusable geometry dan tab hit buffers.
- 178 unit test lolos dan build tanpa warning. PTY membuktikan shared edits/history/save/rename, nested resize/collapse, hidden focus, mouse/drag, syntax union, close-pane multi-doc dan quit dedup. Desain visual tetap terbuka untuk pemolesan tahap berikutnya.

### Workspace satu panel

- Argumen folder/default cwd membuka explorer lazy, folder-first, termasuk dotfiles. Keyboard/mouse expand/open/refresh, wheel per area dan divider 18–40 kolom; narrow screen menyembunyikan explorer sementara.
- Beberapa tab dengan ID stabil, history/view sendiri, dedup dev/inode/canonical alias, marker dirty/read-only dan kontrol overflow/close. Ctrl+N/O menambah tab; Alt+Left/Right atau Ctrl+PageUp/Down berpindah; Ctrl+W close dan Ctrl+Q memeriksa semua dirty.
- Save As ke target tab lain ditolak. Dialog bertarget ID; dirty tab nonaktif ditampilkan sebelum close; Save As cancel/failure menghentikan close/quit. Discard per dokumen tidak memalsukan checkpoint saved.
- Ctrl+F Find, **Ctrl+R Replace** (Ctrl+H tetap Backspace), F3/Shift+F3 wrap, F6 replace-current tervalidasi dan F7 Replace All satu undo. Literal case-sensitive, grapheme-safe, Unicode/multiline; field 4096 byte, jendela hasil komposisi 64 MiB.
- Syntect menjadi dependency runtime, semantic highlighting dengan checkpoint 128 baris, budget 200 baris/update, token viewport dan invalidasi generation termasuk undo/redo. Batas 16 KiB/baris/error parser menghasilkan warning dan plaintext, bukan token stale.
- Tema Dark/Light/Terminal, auto/truecolor/256/16/none serta Unicode/Nerd/ASCII tersedia melalui CLI. Palette RGB/xterm-256 diuji kontras; mono mempertahankan fokus/seleksi non-color. Nerd Codicons opt-in, font tidak dipasang/dideteksi.
- Perbaikan ellipsis UTF-8 satu kolom, traversal child/ancestor symlink, scroll explorer pasca-redraw dan klik terhadap frame stale. Geometry layout dipertahankan terpisah dari validitas hit.
- Batch workspace satu panel sebelumnya lolos 140 unit test dan PTY tab/explorer/resize, search/replace/undo, dirty/save, syntax dan matriks tema; cakupan hybrid terbaru tercatat di atas dan [ROADMAP.md](docs/ROADMAP.md).

### Editor satu dokumen

- `notefart` / `cargo run` kini membuka editor kosong; argumen file membuka dokumen atau menyiapkan path baru tanpa membuat file sebelum Save. `--help`, `--version`, `-- FILE` dan `--probe` tetap tersedia.
- Document berbasis Ropey, delta undo/redo dan saved checkpoint dengan state ID monotonik. Navigasi/delete, word selection, cursor dan hit testing menghormati grapheme, chunk seam, combining mark, CRLF dan sel terminal.
- Editing langsung, seleksi keyboard/mouse, klik/double-click/drag/autoscroll, navigasi dan clipboard **internal**. Ctrl+C tidak keluar dari editor; Ctrl+Q/W memakai Save/Discard/Cancel. Paste valid satu transaksi undo.
- New/Open dialog, Ctrl+S Save, F4 Save As dan konfirmasi target existing termasuk path dokumen saat ini. Save gagal/cancel tidak menyelesaikan permintaan keluar atau membersihkan dirty state.
- UTF-8/BOM/LF/CRLF round-trip; mixed/lone CR read-only tanpa normalisasi implisit. Invalid UTF-8, NUL dan non-regular file ditolak. Batas load/save 64 MiB.
- Fingerprint isi + identitas/metadata mendeteksi konflik eksternal, dengan overwrite eksplisit, Save As, Reload/discard lokal, dan Cancel.
- Tempfile satu direktori, atomic replacement/no-clobber, file dan directory sync; mode/uid/gid/xattrs/ACL dipertahankan atau gagal eksplisit. Metadata keamanan diperiksa ulang meski overwrite diizinkan. File baru tetap 0600 tanpa mengubah umask proses; save langsung ke symlink/hard link ditolak.
- UI Dark dengan header, nomor baris, seleksi, cursor, status dan shortcut helper. Gutter yang berubah ikut menjaga cursor terlihat; dialog kurang dari 20×6 menawarkan resize/Esc dan menolak keputusan tersembunyi.

### Cutover input terminal

- Editor dan probe memakai decoder streaming berbatas pada thread utama; thread pembaca dan decoder event Crossterm lama dihapus.
- Batas paste 1 MiB; overflow/invalid UTF-8 dibuang utuh sampai terminator, tanpa menjalankan kontrol di payload. CSI/UTF-8/paste fragment tetap aman melintasi polling kosong; parser meresinkronisasi ESC baru, mengabaikan OSC/string-control sampai terminator dan mendukung SGR mouse.
- `--probe` tetap alat diagnostik input, resize dan Unicode. Ctrl+C hanya keluar dalam mode probe; editor memakainya untuk copy internal.
- Contoh `text_probe` dan `highlight_probe` tetap alat evaluasi terpisah; editor memakai engine produksi sendiri.

### Verifikasi dan batas

- Batch editor satu dokumen sebelumnya lolos 70 test; workspace satu panel 140; hybrid shared-view 178; polish field/help 204; redesign visual terbaru 217. Bukti tiap tahap dan batas lingkungan ada di [ROADMAP.md](docs/ROADMAP.md).
- Probe dengan decoder baru diuji melalui tmux 3.5a terisolasi; fixture dan server pengujian sudah dibersihkan. Detail ada di [ROADMAP.md](docs/ROADMAP.md).
- Belum tersedia: clipboard sistem/OSC 52, autosave/crash recovery dan konfigurasi persisten. UI/UX masih tahap pemolesan; glyph/font fisik/Nerd Font belum dibuktikan.
- Batas lain: filesystem sinkron lambat, final check→rename race, matriks ACL/security label, panic/syscall fault injection, suspend eksternal, SSH, OS/font/protokol lain. Tidak ada klaim keamanan/performa universal; lihat [SECURITY.md](docs/SECURITY.md).
