# Audit UI/UX dan Accessibility End-to-End — Notefart

> **Cakupan audit:** Tinjauan read-only terhadap seluruh alur pengguna dan kode sumber (`src/*.rs`, `docs/*.md`, `Cargo.toml`, `config.toml.example`) tanpa mengubah kode aplikasi maupun perilaku runtime.
>
> **Konteks produk:** Penilaian diselaraskan dengan arah produk Notefart di [PRODUCT.md](PRODUCT.md) dan [DESIGN.md](DESIGN.md): editor teks terminal ala Notepad (non-modal, bukan IDE), tampilan terinspirasi *superfile*, serta dukungan penuh keyboard dan mouse.

## Status tindak lanjut batch P0

Bagian audit di bawah dipertahankan sebagai catatan kondisi awal; nomor baris sumbernya mengacu pada snapshot saat audit. Status implementasi setelah perbaikan:

| Temuan | Status | Hasil |
|---|---|---|
| 1.1–1.3 | Diperbaiki | Border editor aktif, indikator masalah explorer, dan canvas compact memakai warna eksplisit dari token tema. |
| 2.1, kontras pilihan explorer | Diperbaiki | Marker pada baris terpilih memakai foreground seleksi yang terbaca; palet tema tidak diganti. Usulan theme switcher tetap di luar batch. |
| 1.4 | Recovery aman tersedia; ambiguitas legacy tetap | Urutan kontrol yang belum selesai menampilkan petunjuk `Ctrl+G`. BEL/ST menyelesaikan drain tanpa menjalankan payload sebagai shortcut. Tidak ada timeout yang menebak `Alt+]` sebagai tombol biasa. |
| 1.5 | Diperbaiki | Open/Save As yang gagal memulihkan teks, caret, dan seleksi path; retry mempertahankan kelanjutan Close/Quit sampai save sukses atau dibatalkan. Termasuk kegagalan setelah konfirmasi overwrite. |
| 1.6 | Kontrak diperjelas, bukan diubah | Discard menutup dokumen tersebut langsung. Cancel berikutnya tidak membatalkan Discard terdahulu; dialog kini menyatakannya secara eksplisit. |
| 1.7–1.8 | Diperbaiki | Semua pesan dan pilihan harus muat sebelum mnemonic destruktif aktif. Jika tidak muat, hanya Resize/Esc; notice terminal kecil tetap menampilkan cara keluar. |
| 1.9 | Diperbaiki | Level status eksplisit, warning highlight terpisah dan persisten per dokumen, serta Undo/Redo read-only memberi warning tanpa mengedit. |
| 1.10 | Diperbaiki | Copy System yang gagal tidak menjanjikan fallback clipboard internal. Scroll horizontal dibatasi isi baris yang terlihat dan ruang caret akhir baris. |
| 3.4.4 | Sesuai keputusan pengguna | `naf` menampilkan explorer, `naf FILE` menyembunyikannya, `naf FOLDER` menampilkannya; `Ctrl+B` tetap toggle. Tidak dikembalikan ke perilaku lama. |
| Usulan P1/P2 lainnya | Di luar batch P0 | Sebagian P1 disetujui pada batch lanjutan di bawah; item lain tetap usulan/desain terpisah. |

Bukti build, suite ATMO, runtime TUI, hash binary, dan keterbatasan dicatat di [ROADMAP — hasil perbaikan P0](ROADMAP.md#hasil-perbaikan-bug-p0-audit-agy). Verifikasi input security dikerjakan oleh worker `atmo/alicn/deepseek-v4.1-flash-1017`, bukan integrator.

## Status tindak lanjut batch P1 terpilih

Batch P1 terpilih telah terintegrasi dan lulus verifikasi fungsional `atmo/alicn/deepseek-v4.1-flash-1017` pada aplikasi nyata. Bukti penutupan dan batas tahap 4 dicatat di [ROADMAP](ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian); ini tidak menaikkan seluruh usulan P1/P2 audit menjadi fitur tersedia.

| Bagian audit | Cakupan yang disetujui | Status |
|---|---|---|
| 3.1 helper; 2.4 overflow tab | Prioritas Help/All, Quit, Files, Pane pada 80 kolom; hit panah disabled dihapus | Lulus unit/runtime |
| 2.2 resize explorer dan tombol konfirmasi | Alt+Shift+Left/Right saat explorer fokus; Tab/panah/Enter/Space dengan Cancel default dan return F1 | Lulus unit/runtime |
| 3.2.1–2 | Prefill seleksi satu baris, posisi/total match berbatas jujur dan wrapped, cache revisi, overlap Find dua arah | Lulus unit/runtime |
| 3.3.1, konteks path | Path target/Untitled pada Unsaved/Overwrite/Conflict, termasuk guard ukuran | Lulus unit/runtime |
| P1/P2 di luar cakupan di atas | Misalnya keymap runtime serta hover/toleransi double-click field | Tetap usulan; indent/dedent, scrollbar, indikator field, Appearance serta ergonomi field/navigasi Explorer ditangani batch lanjutan di bawah |

Dua review fungsional read-only AGY telah selesai. Temuan invalidasi cache saat reload, konsistensi overlap maju dan hit mouse stale saat membuka pencarian ditangani integrator dengan regresi tambahan. Suite 310 tes dan seluruh skenario runtime P1 di atas lolos; temuan override warna backend diselesaikan dengan regresi dan uji ulang terarah. Pengguna menyetujui penutupan tahap 4 untuk matriks Linux teruji dengan batas fisik/non-loopback yang dicatat di ROADMAP.

## Status tindak lanjut indent/dedent blok

- Usulan 3.5.1 telah diimplementasikan: Tab dengan seleksi multibaris mengindentasi prefix, Shift+Tab melakukan dedent baris/blok; tidak lagi menghapus isi seleksi multibaris.
- Arah seleksi, endpoint awal baris berikutnya, grapheme, tab/spasi asli, satu undo, shared view dan no-op/redo dibuktikan oleh tes serta TUI nyata.
- DeepSeek menjalankan 113 tes terarah dan 23 skenario live; review read-only terpisah selesai tanpa temuan defect source-level. Review dialihkan ke DeepSeek atas persetujuan pengguna karena AGY tidak tersedia; tidak ada hasil review AGY untuk batch ini.
- [Metode, hash release dan batas](ROADMAP.md#hasil-indent-dan-dedent-blok). Scrollbar, runtime keymap/theme serta usulan editing lain tetap di luar batch.

## Status tindak lanjut scrollbar editor

- Usulan 3.4 untuk body editor ditangani dengan track/thumb vertikal dan horizontal per view, terpisah dari border resize. Klik page dan drag mengubah viewport tanpa fokus/cursor/seleksi atau transaksi edit.
- Pengukuran horizontal mengikuti lebar render baris yang terlihat dan berjalan bertahap; state pending, minimum panel dan background terminal mengikuti [kontrak scrollbar](ARCHITECTURE.md#scrollbar-editor).
- Scrollbar explorer/help, indikator overflow field, runtime switcher serta usulan P2 lain tidak menjadi fitur tersedia melalui batch ini. Isi audit awal di bawah tetap catatan historis, bukan daftar kekurangan implementasi terbaru.
- Recheck akhir DeepSeek lulus: 88 tes terarah dan TUI nyata setelah perbaikan rail minimum, endpoint thumb terkuantisasi serta resize pane nonaktif. Tidak ada klaim review AGY baru; [bukti, hash dan batas](ROADMAP.md#hasil-scrollbar-editor).

## Status tindak lanjut scrollbar Explorer dan F1

- Explorer dan katalog F1 kini memiliki rail vertikal, klik page dan drag thumb dengan primitive yang sama seperti editor. Parent row/divider tree serta footer bantuan bukan bagian rail.
- Scroll tree tidak mengganti pilihan, fokus atau dokumen; scroll F1 tidak mengubah field Find/Replace, Open/Save As maupun state konfirmasi yang ditangguhkan. Wrapping, page dan limit bantuan memakai ruang katalog setelah rail.
- DeepSeek memverifikasi 130 kasus tes berbeda pada empat filter terarah serta TUI nyata, termasuk minimum/tiny, resize-held drag, modal return dan fallback ikon. Bukan suite seluruh target atau review AGY baru.
- [Kontrak](ARCHITECTURE.md#scrollbar-explorer-dan-f1) dan [bukti, hash serta batas](ROADMAP.md#hasil-scrollbar-explorer-dan-f1). Indikator overflow field dan switcher runtime tetap di luar batch; isi audit awal di bawah tidak ditulis ulang.

## Status tindak lanjut indikator overflow field

- Path, Find dan Replace memakai indikator pasif `‹`/`›`, atau `<`/`>` pada ASCII, saat window menyembunyikan teks. Indikator tidak dimasukkan ke `FieldHit`, sehingga kliknya tidak menempatkan caret atau memindahkan fokus ke field lain.
- Dua gutter hanya dibuat jika tersedia minimal tiga sel nilai; pada satu/dua sel, caret diprioritaskan. Ini tidak menambahkan autoscroll, undo field, word-delete, toleransi double-click atau hover dari usulan audit.
- [Kontrak field dan mapping byte](ARCHITECTURE.md#keputusan-pemolesan-field-dan-bantuan). Isi audit awal tetap catatan historis, bukan daftar kekurangan terbaru.
- Tiga kasus tes terarah lulus setelah migrasi assertion caret; smoke penutupan `openai-codex/gpt-6.1-terra` membuktikan byte klik setelah resize, seleksi path, clipboard penuh dan return F1 melalui output fixture. Bukan review AGY atau security audit baru; [bukti, hash dan batas](ROADMAP.md#hasil-indikator-overflow-field).

## Status tindak lanjut switcher tampilan runtime

- F11/Alt+P membuka draft tema, mode warna dan profil ikon dengan Apply/Cancel, keyboard dan mouse. F1 menangguhkan lalu memulihkan draft/fokus; F12 tetap Close pane.
- Apply berlaku untuk run ini tanpa menyimpan konfigurasi/sesi, mengganti backend clipboard/font, mengubah dokumen/undo atau mengikuti ulang caret saat menu ditutup. Background utama tetap mengikuti terminal.
- Ukuran minimum 40×10; di bawahnya hanya Esc/F1 yang aktif dan target klik lama tidak dipakai. Ini bukan konfigurasi keymap atau preview tema.
- [Kontrak switcher](ARCHITECTURE.md#kontrak-switcher-tampilan-runtime). Isi audit awal tetap catatan historis, bukan daftar kekurangan terbaru.
- Empat tes Appearance lulus. Smoke awal terhenti oleh selector driver yang keliru, bukan bug aplikasi; smoke sisa skenario lulus tanpa mengubah produksi atau mengulang tes. Penutupan memakai `openai-codex/gpt-6.1-terra`, bukan review AGY/security baru; [bukti, hash dan batas](ROADMAP.md#hasil-switcher-tampilan-runtime).

## Status tindak lanjut ergonomi field dan navigasi Explorer

- Field path/Find/Replace memiliki undo/redo delta lokal, penghapusan kata dan drag autoscroll milik field asal. F1 menjaga history, dan revision query tetap monoton tanpa mengubah dokumen belakang.
- Type-ahead jump memakai prefix pada row ter-load, wrap dan timeout 900 ms, tanpa memfilter atau memindai ulang filesystem. Kartu error root tetap menyediakan F5 Retry, dengan Bksp Up hanya bila parent diketahui; issue entri terpilih tampak pada status.
- Ctrl+Backspace/Ctrl+Shift+Z memakai dukungan xterm modifyOtherKeys; byte Ctrl+H legacy tetap Backspace. Implementasi protokol dikerjakan DeepSeek via atmo, bukan GPT/AGY.
- [Kontrak integrasi dan batas](ARCHITECTURE.md#kontrak-ergonomi-field-dan-navigasi-explorer). Catatan audit awal tidak diubah menjadi klaim kekurangan terbaru; keymap runtime, hover dan toleransi double-click field tetap terpisah.
- Sebanyak 24 kasus target berbeda serta smoke TUI lulus, termasuk recheck affordance root `.` dari cwd terhapus. QA normal oleh Terra Codex; input/negative dan permission oleh DeepSeek atmo, bukan review AGY baru atau audit seluruh repo. [Bukti dan batas](ROADMAP.md#hasil-ergonomi-field-dan-navigasi-explorer).







---

## Ringkasan Eksekutif

Fondasi arsitektur UI/UX dan keamanan visual Notefart sudah sangat matang pada beberapa aspek inti:
- **Integritas & keamanan rendering:** Escape sequence terminal, karakter Bidi, tab, dan karakter zero-width pada isi dokumen (`LineCells::next` di [src/document.rs](../src/document.rs#L783-L872)) serta field modal (`field_ui::sanitize_grapheme` di [src/field_ui.rs](../src/field_ui.rs#L21-L35)) dinetralkan secara konsisten dan selaras antara render, kursor, dan *hit-testing*.
- **Ketahanan terminal & layout:** Lifecycle raw mode/alternate screen/mouse/bracketed paste terpusat dengan pemulihan signal/panic ([src/terminal.rs](../src/terminal.rs#L116-L274)), serta layout responsif dari mode framed penuh hingga compact `12×6` tanpa menghapus state split/dokumen.
- **Disiplin UX dasar:** Clipboard jujur saat fallback ke internal (mis. di sesi SSH), `F1` Help menangguhkan modal tanpa menghilangkan isi field, dan `Ctrl+C` tanpa seleksi tidak keluar paksa.

Namun, audit end-to-end menemukan **sejumlah bug visual/interaksi nyata**, **lubang aksesibilitas (kontras WCAG, keyboard-only navigation, indikator fokus/scroll)**, dan **friksi alur kerja harian** yang cukup signifikan untuk segera dipoles.

| Kategori | Kondisi Saat Ini | Risiko Utama |
| --- | --- | --- |
| **Visual & Kontras (A11y)** | Truecolor Dark/Light lolos test kontras 4.5:1, tetapi ada bug warna hilang di UI, 2 pasangan warna UI tidak diuji, dan `Ansi256` melanggar 4.5:1 pada badge/marker terpilih. | Border panel aktif dan ikon warning `!` di Light theme bisa tak terlihat; marker `>` dan `!` di `Ansi256` di bawah 4.5:1. |
| **Keyboard-Only Access (A11y)** | Mayoritas alur inti bisa via keyboard, tetapi beberapa aksi hanya via mouse dan tombol konfirmasi tidak mendukung `Tab`/panah. | Pengguna tanpa mouse tidak bisa resize explorer, melompat ke tab overflow, atau mengganti tema/ikon saat runtime; shortcut bentrok terminal/tmux belum punya alternatif. |
| **Kejelasan State & Orientasi** | Tidak ada scrollbar/indikator posisi di mana pun; collapse split tanpa tanda visual; helper 80 kolom memotong aksi penting (`Quit`, `Files`, `Pane`). | Pengguna sulit mengetahui posisi scroll, keberadaan panel tersembunyi, atau alasan pesan status/error yang tertimpa. |
| **Alur Dialog & Editing** | Open/Save As membuang input saat gagal; Find/Replace tanpa jumlah match (`X of Y`) dan tanpa prefill seleksi; `Modal::Unsaved` tidak menyebut path/sisa file. | Salah ketik path memaksa mengetik ulang dari nol; multi-close/quit berisiko partial discard jika dibatalkan di tengah. |

---

## 1. Bug & Masalah Kritis (Prioritas Tinggi)

Masalah berikut merupakan ketidaksesuaian langsung antara perilaku kode dengan desain ([DESIGN.md](DESIGN.md)) atau potensi hilangnya input/state pengguna.

### 1.1. Hilangnya Warna `accent` pada Border Panel Editor Aktif
- **Lokasi:** `draw_pane_card` di [src/ui.rs](../src/ui.rs#L1121-L1126)
- **Temuan:** Saat `pane_focused == true`, `border_style` diset `Style::default().bg(theme.canvas).add_modifier(Modifier::BOLD)` **tanpa `.fg(theme.accent)`** (berbeda dari `draw_sidebar` di [src/ui.rs](../src/ui.rs#L833-L836) yang memakai `.fg(theme.accent)`).
- **Dampak:** Melanggar spesifikasi panel aktif di [DESIGN.md](DESIGN.md). Pada tema **Notefart Light** (`canvas = #F5F7FB`) yang dijalankan di terminal berlatar gelap (default `fg` terang), border panel aktif akan berwarna putih/terang di atas background terang sehingga hampir tak terlihat.
- **Saran:** Tambahkan `.fg(theme.accent)` pada `border_style` saat `pane_focused == true`.

### 1.2. Hilangnya Warna `warning` pada Indikator Masalah (`!`) di Explorer
- **Lokasi:** `draw_sidebar` di [src/ui.rs](../src/ui.rs#L1091-L1097)
- **Temuan:** Ketika `entry.issue` ada dan baris **tidak** sedang dipilih (`else`), gaya `" !"` hanya memakai `Style::default().bg(theme.canvas).add_modifier(Modifier::BOLD)` tanpa `.fg(theme.warning)`.
- **Dampak:** Pada **Notefart Light** di terminal gelap, tanda `" !"` nyaris tidak terbaca, padahal ini satu-satunya indikator entri bermasalah (mis. gagal `statat`).
- **Saran:** Tambahkan `.fg(theme.warning)` pada cabang `else`, dan izinkan pengguna melihat teks detail `entry.issue` (mis. di status bar saat baris disorot).

### 1.3. Background Header Mode Compact Bocor ke Warna Terminal
- **Lokasi:** `draw_header` cabang `else` di [src/ui.rs](../src/ui.rs#L758-L809)
- **Temuan:** Pada mode compact, `draw_header` tidak memanggil `fill_row(buf, area, base_style)` (berbeda dengan `draw_tab_bar`, `draw_status`, dan `draw_helper`).
- **Dampak:** Margin kiri/kanan dan celah kosong antara judul dan label `"Notefart"` memakai background terminal bawaan (`Color::Reset`), menimbulkan garis belang warna pada tema Dark/Light.
- **Saran:** Panggil `fill_row(buf, area, base_style);` sebelum menulis ikon dan judul pada cabang compact.

### 1.4. Jebakan Input (`Input Trap`) pada `Alt+]` dan `Alt+Shift+P/X/^/_`
- **Lokasi:** `StreamingDecoder::process_escape_sequence` di [src/input.rs](../src/input.rs#L455-L461) dan `discarding_string` di [src/input.rs](../src/input.rs#L205-L221)
- **Temuan:** Byte kedua `b']' | b'P' | b'^' | b'_' | b'X'` setelah `0x1B` langsung mengaktifkan `self.discarding_string = true` untuk menelan payload OSC/DCS/PM/APC/SOS sampai `BEL` (`0x07`) atau `ST` (`\x1b\\`).
- **Dampak:** Jika pengguna menekan `Alt+]` atau `Alt+Shift+P` (yang dikirim terminal sebagai `\x1b]` / `\x1bP`), **seluruh input keyboard dan mouse berikutnya ditelan tanpa respons** sehingga editor tampak hang total bagi pengguna biasa. Demikian pula `Alt+Shift+O` (`\x1bO`) masuk cabang SS3 (`Incomplete`) hingga tombol berikutnya ditekan.
- **Saran:** Gunakan batas panjang/idle-timeout singkat saat membedakan `Alt+char` dari pembuka OSC/DCS, atau batasi `discarding_string` dengan deadline waktu/byte agar penekanan tombol `Alt` tidak mengunci sesi input selamanya.

### 1.5. Input Path Hilang Saat Open / Save As Gagal
- **Lokasi:** `App::modal_key` untuk `Modal::Path` di [src/app.rs](../src/app.rs#L1315-L1338) dan `App::save_document_to` di [src/app.rs](../src/app.rs#L658-L714)
- **Temuan:** `self.modal.take()` langsung membuang modal sebelum memvalidasi/mengeksekusi `open_tab_from_path` atau `save_document_to`.
- **Dampak:** Jika path salah ketik, direktori belum ada, atau izin ditolak, dialog langsung tertutup dan seluruh path panjang yang sudah diketik hilang; pengguna harus membuka `Ctrl+O`/`F4` dan mengetik ulang dari awal.
- **Saran:** Pertahankan `Modal::Path` tetap terbuka saat `open_tab_from_path` atau `save_document_to` gagal (kecuali saat pindah ke `Modal::Overwrite`/`Modal::Conflict`), dan tampilkan pesan error di dalam dialog atau status bar dengan fokus tetap di field.

### 1.6. Partial Discard Saat Cancel di Tengah Multi-File `ClosePane` (`F12`) / `Quit` (`Ctrl+Q`)
- **Lokasi:** `App::modal_key` untuk `Modal::Unsaved` pilihan `'d'` di [src/app.rs](../src/app.rs#L1397-L1424)
- **Temuan:** Saat user memilih `D Discard` pada dokumen dirty pertama dalam rangkaian `AfterSave::ClosePane` atau `AfterSave::Quit`, dokumen tersebut langsung dihapus dari workspace sebelum memeriksa dokumen dirty berikutnya.
- **Dampak:** Jika di dokumen dirty kedua pengguna menekan `C Cancel` atau `Esc`, panel/aplikasi memang batal ditutup, tetapi **dokumen dirty pertama sudah terlanjur terhapus permanen**.
- **Saran:** Kumpulkan keputusan per dokumen dalam antrean konfirmasi terlebih dahulu sebelum melakukan discard massal, atau nyatakan secara eksplisit di dialog bahwa `Discard` langsung menutup dokumen tersebut (lebih baik opsi pertama demi keamanan data).

### 1.7. Pesan Dialog `Conflict` & `Unsaved` Hilang pada Ukuran Minimum `20×6`
- **Lokasi:** `draw_dialog` cabang `Dialog::Choices` di [src/ui.rs](../src/ui.rs#L2406-L2461)
- **Temuan:** Pada `20×6` (`inner_width = 16`), 5 tombol `ChoiceKind::Conflict` dibungkus menjadi `rows = 4`, sehingga `command_y = rect.y + 1` dan `available = 0`. Seluruh teks pesan `"File changed outside Notefart..."` dilewati (`break`) tanpa dirender sama sekali. Untuk `Unsaved`, hanya 2 baris pesan yang muat dari 3 baris yang dibutuhkan.
- **Saran:** Tingkatkan minimum tinggi modal `Choices` sesuai `message_rows + rows + chrome`, atau ringkas label tombol pada lebar sempit (mis. `[O] Overwrite` -> `[O] Over` saat `< 28` kolom) agar pesan peringatan tidak pernah hilang.

### 1.8. Instruksi Keluar Terpotong pada Layar Sangat Sempit (`< 36` Kolom)
- **Lokasi:** `render_safe_exit_notice` di [src/ui.rs](../src/ui.rs#L525-L565), dan [src/ui.rs](../src/ui.rs#L675-L699)
- **Temuan:** Teks `"Terminal too small (Ctrl+Q to exit)"` dipotong dari ekor oleh `fit_str_with_ellipsis_mode`, sehingga pada lebar `12..35` kolom yang terlihat hanya `"Terminal to…"`.
- **Saran:** Gunakan layout 2 baris saat lebar `< 36` (Baris 1: `"Too small"`, Baris 2: `"Ctrl+Q exit"` atau `"^Q exit"`) agar jalur keluar aman selalu terbaca.

### 1.9. Klasifikasi Status Error Tidak Konsisten & Tertimpa Highlight Worker
- **Lokasi:** `is_error_status`/`is_warning_status` di [src/ui.rs](../src/ui.rs#L1586-L1606), `App::bootstrap_session` di [src/app.rs](../src/app.rs#L257-L261), `App::update_highlight` di [src/app.rs](../src/app.rs#L2753-L2760)
- **Temuan:**
  1. Pesan `"Paste rejected: ..."` ([src/app.rs](../src/app.rs#L2771)), `"Cannot split: ..."` ([src/app.rs](../src/app.rs#L397)), dan `EditError` ([src/app.rs](../src/app.rs#L358)) tidak cocok dengan prefix di `is_error_status`, sehingga tampil seperti pesan biasa tanpa warna `error` maupun prefix `! `.
  2. `bootstrap_session` menggabungkan `"Session restored... | Warn: ..."` sehingga prefix `"Warn"` tidak terdeteksi `is_warning_status`.
  3. `update_highlight` di idle tick menimpa `self.status` secara asinkron jika ada baris >16 KiB atau error parser, sehingga pesan hasil aksi user (`Saved`, `Match at ...`) tiba-tiba hilang.
  4. `Ctrl+Z`/`Ctrl+Y` pada file read-only ([src/app.rs](../src/app.rs#L2100-L2102)) kembali diam-diam tanpa status `"Read-only: ..."`.
- **Saran:** Ganti deteksi berbasis *string prefix* dengan tipe terstruktur `StatusLevel { Info, Success, Warning, Error }` di `App`, dan pisahkan indikator highlight terhenti sebagai badge ringan di status/pane agar tidak menimpa status aksi pengguna.

### 1.10. Pesan Clipboard Gagal Menyesatkan di Mode `System` & Scroll Kanan Tanpa Batas
- **Lokasi:** `Clipboard::copy` di [src/clipboard.rs](../src/clipboard.rs#L192-L218), `copy_failure_status` di [src/app.rs](../src/app.rs#L3024-L3030), `ScrollRight` di [src/app.rs](../src/app.rs#L2607)
- **Temuan:**
  1. Di `ClipboardMode::System`, jika copy gagal, status berbunyi `"Copy failed: ...; data kept in internal clipboard"`. Namun `Ctrl+V` pada mode `System` tetap memanggil `read_external()` ([src/app.rs](../src/app.rs#L2069)) dan gagal lagi—data internal tersebut tidak pernah bisa di-paste kecuali user restart dengan `--clipboard internal`.
  2. Mouse wheel `ScrollRight` melakukan `tab.left_cell.saturating_add(4)` tanpa batas atas terhadap lebar garis terpanjang / viewport, memungkinkan scroll horizontal ke ruang kosong tanpa akhir.
- **Saran:** Jangan mengklaim `"data kept in internal clipboard"` jika mode `System` tidak bisa mem-paste dari internal (atau sediakan fallback/opsi toggle ke internal); batasi `ScrollRight` pada lebar maksimum baris/dokumen yang wajar.

---

## 2. Audit Accessibility (A11y)

### 2.1. Kontras Warna & Degradasi Terminal (`src/theme.rs`, `src/ui.rs`)
- **Pelanggaran target kontras 4.5:1 di `Ansi256`:** `assert_theme_contrasts` ([src/theme.rs](../src/theme.rs#L838-L901)) menguji `canvas` di atas `warning` (yang tidak dipakai widget mana pun), tetapi **melewatkan dua pasangan warna yang benar-benar dirender** di Explorer (`draw_sidebar` [src/ui.rs](../src/ui.rs#L1016) dan [src/ui.rs](../src/ui.rs#L1092)):
  - `accent` di atas `selection` (marker fokus `>` pada baris terpilih):
    - `PALETTE_DARK_256`: `117` (`#87D7FF`) di atas `24` (`#005F87`) ≈ **4.42:1 (< 4.5:1)**
    - `PALETTE_LIGHT_256`: `25` (`#005FAF`) di atas `153` (`#AFD7FF`) ≈ **4.30:1 (< 4.5:1)**
  - `warning` di atas `selection` (tanda ` !` pada baris terpilih):
    - `PALETTE_LIGHT_256`: `94` (`#875F00`) di atas `153` (`#AFD7FF`) ≈ **3.82:1 (< 4.5:1)**
- **Keterbatasan `Ansi16`, `Terminal`, dan `ColorMode::None`:**
  - `PALETTE_DARK_16`, `PALETTE_LIGHT_16`, dan `PALETTE_TERMINAL` ([src/theme.rs](../src/theme.rs#L410-L474)) memakai `Color::DarkGray` untuk `muted`, `border`, dan `syntax.comment`, yang di banyak tema terminal klasik memiliki kontras sangat rendah terhadap hitam maupun putih.
  - `ThemeKind::Terminal` ([src/theme.rs](../src/theme.rs#L211)) diam-diam mengabaikan `Truecolor`/`Ansi256` dan memaksa `PALETTE_TERMINAL`.
  - Di `ColorMode::None`, seluruh 13 token syntax ([src/theme.rs](../src/theme.rs#L222-L237)) tampil identik tanpa pembeda tipografi (`BOLD`/`ITALIC`/`UNDERLINED` minimal untuk heading/keyword/comment).
- **Tidak ada Runtime Switcher:** Pengguna dengan sensitivitas cahaya atau low-vision harus menutup editor dan mengubah CLI flag / `config.toml` untuk berpindah antara Dark, Light, Terminal, `color=none`, atau `icons=ascii`.
- **Saran:**
  1. Perbaiki indeks `selection`/`accent`/`warning` pada `PALETTE_DARK_256` dan `PALETTE_LIGHT_256` (mis. gunakan `selection` yang lebih gelap di Dark seperti `23`/`17` atau `accent` lebih terang, dan `warning` lebih gelap di Light seperti `58`/`88`), serta tambahkan `accent on selection` dan `warning on selection` ke `assert_theme_contrasts`.
  2. Sediakan aksi/menu ringkas atau command/shortcut untuk mengganti `theme` (Dark/Light/Terminal) dan `icons` saat runtime tanpa restart.

### 2.2. Aksesibilitas Keyboard-Only (Motor & Navigasi Tanpa Mouse)
- **Aksi yang masih *mouse-only* atau tidak punya jalur keyboard langsung:**
  1. **Resize lebar sidebar Explorer:** `DragState::ExplorerDivider` hanya bisa digeser dengan mouse; `App::resize_nearest_divider` ([src/app.rs](../src/app.rs#L433-L491)) mengabaikan divider explorer (`Alt+Shift+Left/Right` tidak berfungsi untuk sidebar).
  2. **Navigasi tombol dialog konfirmasi (`Modal::Unsaved`, `Overwrite`, `Conflict`):** `App::modal_key` ([src/app.rs](../src/app.rs#L1380-L1440)) hanya menerima huruf mnemonic (`s`/`d`/`c`, `y`/`n`/`c`, `o`/`a`/`r`/`c`), mengabaikan `Tab`, `Shift+Tab`, `Left`/`Right`, `Space`, dan `Enter`. Ini tidak familiar bagi pengguna keyboard yang terbiasa berpindah tombol dengan `Tab`/panah.
  3. **Memilih baris `..` di Explorer via panah:** Baris `..` hanya di-hit oleh mouse atau tombol `Backspace`; menekan `Up`/`Home` tidak bisa mencapai `..` untuk ditekan `Enter`.
  4. **Toggle expand/collapse inline di Explorer tanpa masuk folder:** Klik mouse me-toggle expand/collapse, sedangkan keyboard `Enter` me-root ke folder. Tombol `Space` belum dipakai dan ideal untuk toggle expand/collapse inline (serta `Right` pada folder terbuka pindah ke child pertama; `Left` pada root item tertutup naik ke parent).
  5. **Lompat ke tab atau panel tertentu:** Ketika banyak tab overflow di satu pane, pengguna keyboard hanya bisa menekan `Alt+Left/Right` berulang kali.
- **Bentrok Shortcut Lingkungan Terminal & OS:**
  - `F1` (Help terminal GNOME/XFCE), `F10` (Menu terminal GNOME/XFCE/Tilix), `Ctrl+W`/`Ctrl+R` (Tab/Refresh terminal browser), `Ctrl+S`/`Ctrl+Q` (XOFF/XON pada serial/SSH tertentu), `Ctrl+PageUp/Down` (Tab emulator terminal), `Ctrl+B` (prefix `tmux`), dan `Ctrl+A` (prefix `screen`) semuanya belum memiliki alternatif maupun konfigurasi keybinding.
  - Tidak ada dukungan *Kitty keyboard protocol* (`PushKeyboardEnhancementFlags`) pada terminal modern, sehingga `Ctrl+H` vs `Backspace`, `Ctrl+Shift+Z`, dan `Ctrl+Tab` tidak dapat dibedakan.
- **Saran:**
  1. Dukung `Alt+Shift+Left/Right` untuk resize Explorer saat Explorer fokus (atau saat fokus di pane paling kiri dekat divider).
  2. Tambahkan navigasi `Tab`/`Shift+Tab`/`Left`/`Right` dengan indikator fokus tombol pada `Dialog::Choices` (tetap pertahankan mnemonic huruf; jangan jadikan aksi destruktif sebagai tombol default `Enter`).
  3. Pertimbangkan alternatif shortcut ramah terminal/60%-keyboard (mis. `Alt+H` atau `F1` untuk Help, `Alt+P`/`Ctrl+Tab` jika protokol tersedia, atau keymap minimal di `config.toml`).

### 2.3. Kejelasan Fokus, Status Kritis & Indikator Posisi (Low-Vision & Kognitif)
- **Hilangnya penanda panel target saat Explorer fokus:** Di [src/ui.rs](../src/ui.rs#L609), `pane_focused = is_active_pane && !model.explorer_focused`. Saat Explorer fokus pada layout split, semua panel editor tampil dengan border non-aktif yang sama, sehingga pengguna tidak bisa melihat panel editor mana yang akan menerima file yang dibuka dari Explorer.
  - *Saran:* Pertahankan penanda sekunder yang tenang pada `active_pane_id` saat Explorer fokus (mis. judul `"Editor (target)"` atau ikon aksen tanpa marker fokus penuh `●`).
- **Tab aktif di pane tidak fokus lemah:** Pada mode Truecolor/`Ansi256`, ketika `!pane_focused` ([src/ui.rs](../src/ui.rs#L1255-L1257)), tab aktif tidak memakai `BOLD` maupun `REVERSED`, hanya teks `fg` vs `muted` dan 1 karakter `·`. Ini sulit dibaca pada layar kontras rendah atau saat membandingkan dua panel.
- **Collapse split tanpa indikator visual:** Ketika layar menyempit dan `SplitTree::layout` menyembunyikan panel non-aktif (`geometry.collapsed == true` di [src/ui.rs](../src/ui.rs#L647)), tidak ada tanda visual apa pun (mis. `"Pane 1/2"` atau `"[+1 hidden]"` di judul/status).
- **Fokus Explorer tertahan saat Explorer tersembunyi di layar sempit:** Jika lebar terminal mengecil saat `explorer_focused == true`, kursor editor muncul di layar ([src/ui.rs](../src/ui.rs#L668-L671)), namun input keyboard ditolak dengan pesan `"Explorer hidden on narrow screen..."` ([src/app.rs](../src/app.rs#L1827-L1834)). Sebaiknya kembalikan fokus sementara ke editor (atau tampilkan indikator jelas) saat sidebar tersembunyi otomatis.
- **Indikator `[RO]` diam-diam hilang pada tab sempit:** Di [src/ui.rs](../src/ui.rs#L1330), `[RO]` hanya digambar jika `content_w >= dirty_w + 6`. Pada panel sempit, label `[RO]` hilang total tanpa fallback `"R"`.
- **Absennya indikator scroll & overflow di semua panel:**
  - **Editor body:** Tidak ada scrollbar vertikal/horizontal, persentase posisi, maupun indikator tepi `◀`/`▶` saat baris panjang ter-scroll horizontal (`left_cell > 0`).
  - **Explorer & Help (`F1`):** Tidak ada scrollbar maupun penanda `"↑ more"` / `"↓ more"` atau `"Line X/Y"`, padahal keduanya dapat di-scroll panjang.
  - **Text Field modal ([src/field_ui.rs](../src/field_ui.rs)):** Marker `…` hanya muncul jika *sebuah glyph multi-kolom terpotong* di tepi window; teks ASCII panjang yang terpotong ke kiri/kanan tidak memberi tanda `‹` / `›` bahwa ada teks di luar viewport.
- **Sufiks fokus judul panel terpotong di panel sempit (`12..15` kolom):** `" ✎ Editor ● "` (12 kolom) terpotong di panel lebar 12 (`inner = 10`), menghilangkan `"●"`. Letakkan marker `"●"` di depan atau persingkat judul menjadi `"● Editor"` / `"●"` pada panel `< 15` kolom.

### 2.4. Ergonomi Mouse & Target Klik (`HitTarget`)
- **Tombol overflow tab (`[‹]`/`[›]`) tampak non-aktif tetapi tetap berfungsi:** Di [src/ui.rs](../src/ui.rs#L1530-L1583), saat `start == 0` atau `end + 1 == tabs.len()`, panah digambar warna `theme.muted`, namun hit rect tetap didaftarkan sehingga klik di atasnya tetap melakukan `cycle_tab` dengan *wrap-around* ([src/app.rs](../src/app.rs#L2314-L2350)).
- **Target klik 1×1 sel & absennya affordance divider:** Tombol close tab (`×`) hanya 1×1 sel dan langsung bersebelahan dengan body tab; divider split/explorer hanya menampilkan 1 sel grip (`┊`/`⋯`) di tengah, sisanya spasi kosong sehingga pengguna baru tidak tahu seluruh garis gap dapat di-drag.
- **Double-click & drag yang kaku:**
  - Double-click mensyaratkan koordinat sel yang **identik 100%** antara klik pertama dan kedua (`x == mouse.column && y == mouse.row` di [src/app.rs](../src/app.rs#L2460-L2461)); toleransi 1 sel sangat membantu bagi pengguna touchpad/tremor.
  - Modal `Field` tidak mendukung double-click pilih kata dan **tidak mendukung autoscroll horizontal saat drag** ([src/app.rs](../src/app.rs#L2239-L2263)).
  - Klik pada label prefix field (`"> Find: "` / `"  Repl: "`, [src/field_ui.rs](../src/field_ui.rs#L89)) diabaikan alih-alih memindahkan fokus ke field tersebut.
- **Tidak ada feedback hover:** `MouseEventKind::Moved` diparse oleh `input.rs` tetapi diabaikan seluruhnya oleh `app.rs`. Highlight ringan saat pointer berada di atas tombol `×`, chip helper, atau divider drag akan sangat meningkatkan *discoverability* mouse.

### 2.5. Sanitasi Chrome, Ikon & Pesan TTY
- **Celah zero-width pada `ui::sanitize_ui_cow` ([src/ui.rs](../src/ui.rs#L210-L235)):** Tidak seperti `document.rs` dan `field_ui.rs`, `sanitize_ui_cow` hanya menyaring kontrol & daftar Bidi tertentu—zero-width char lain (mis. `U+200C`, `U+200D`, `U+00AD`) lolos di nama file/tab/status lalu di-skip oleh `write_clipped_str`, berisiko menimbulkan perbedaan visual nama file (*homograph*).
- **Lebar ambigu Nerd glyph & pembeda kategori:** `icon_width_bounds_and_kind_precedence` ([src/theme.rs](../src/theme.rs#L1060-L1183)) belum menguji `width_cjk` untuk Nerd `chevron` dan `close_icon`; selain itu, pada mode Nerd, `FileCategory::Generic` dan `Binary` memakai glyph yang sama (`NERD_FILE`) dan hanya dibedakan warna.
- **Pesan error `verify_tty` ([src/terminal.rs](../src/terminal.rs#L213), [src/terminal.rs](../src/terminal.rs#L219)):** Menyebut `"--probe requires an interactive terminal"` meski dipanggil saat startup editor normal `naf`.

---

## 3. Audit Alur Kerja UX End-to-End (Saran Peningkatan)

### 3.1. Helper Bar & Katalog Bantuan (`F1`, `src/shortcuts.rs`, `src/ui.rs`)
- **Pemotongan Helper Bar pada lebar standar 80 kolom:** `EDITOR_HELPER_ACTIONS` ([src/ui.rs](../src/ui.rs#L1767-L1801)) memiliki 8 item. Pada terminal **80×24**, item ke-6 s.d. ke-8 (**`[Ctrl+Q] Quit`**, **`[F2] Files`**, **`[F10] Pane`**) **hilang terpotong** tanpa tanda `+3` / `[F1] More`.
  - *Saran:* Urutkan berdasarkan prioritas keluar/navigasi (atau tampilkan versi ringkas seperti `^Q Quit` / `[F1] All` di ujung kanan) agar `[Ctrl+Q] Quit` dan `[F2] Files` selalu muat di 80 kolom.
- **Sinkronisasi katalog `shortcuts.rs` dengan kode aktual:** Tambahkan shortcut yang sudah ada di kode tetapi belum tercatat di `SECTIONS` ([src/shortcuts.rs](../src/shortcuts.rs#L8-L229)): `Ctrl+F`/`Ctrl+R` di dalam modal Find/Replace untuk switch mode, `Ctrl+Home`/`Ctrl+End` di `Field`, `Esc` di editor untuk *clear selection*, `Shift+Click`, serta rincian tombol `Overwrite` (`Y`/`N`) dan `Conflict` (`O`/`A`/`R`/`C`).
- **Hint modal pada `Dialog::Search` kecil (`20×6..39×9`):** Saat dialog menampilkan `"Resize!\nEsc back"`, bottom helper masih menampilkan `"Shift+Arrows Select ^A All ^C/X/V Clipboard"` padahal field diblokir. Sembunyikan hint field saat modal terlalu kecil dan tambahkan `CommandHit` klikable pada `[Esc] Back` seperti di `help::draw_too_small`.

### 3.2. Find & Replace (`src/search.rs`, `src/app.rs`, `src/ui.rs`)
1. **Prefill dari seleksi editor:** Saat pengguna menyeleksi kata (satu baris) lalu menekan `Ctrl+F` atau `Ctrl+R`, isi field `Find` otomatis dengan teks seleksi tersebut (sangat standar di editor desktop ala Notepad/VS Code).
2. **Penghitung Match & Indikator Wrap:** Tampilkan `"Match 3 of 12"` (atau batas seperti `"Match 3 of 100+"` bila menjaga performa file besar) dan keterangan `"(wrapped)"` saat pencarian berputar dari bawah ke atas atau sebaliknya.
3. **Bedakan highlight Match Aktif / Match Lain vs Seleksi Biasa:** Saat dialog Find/Replace terbuka dan fokus berada di field input, sulit membedakan seleksi match aktif dari seleksi biasa (terutama bila match tertutup dialog popup di tengah layar). Geser viewport atau posisi dialog agar match tetap terlihat, dan beri style khusus (mis. `warning` subtle / underline) untuk match aktif serta highlight ringan untuk match lain di viewport.
4. **Alur `F6` (Replace current):** Jika user baru mengetik query baru di Replace lalu menekan `F6`, alih-alih menolak dengan error, jalankan `find` ke match pertama lalu tampilkan/ganti sesuai ekspektasi yang jelas.
5. **Toggle Case-Sensitive:** Tambahkan opsi sederhana `[Alt+C] Match Case` (default tetap case-sensitive atau sesuai preferensi) tanpa menambah kompleksitas regex/IDE.

### 3.3. Dialog Konfirmasi & Text Field (`src/field.rs`, `src/app.rs`, `src/ui.rs`)
1. **Konteks file pada dialog konfirmasi:**
   - `Modal::Unsaved` ([src/app.rs](../src/app.rs#L2868-L2874)): Sertakan path relatif/lengkap (ter-elisi tengah) serta progres saat multi-dokumen, mis. `"Save changes to src/main.rs? (1 of 3 unsaved)"`.
   - `Modal::Overwrite` & `Modal::Conflict`: Sertakan path file target pada pesan dialog.
   - Beri warna semantik yang berbeda pada tombol aksi destruktif (mis. `D Discard` / `Y Overwrite` memakai `error` atau `warning`, sementara `S Save` / `C Cancel` memakai `accent`/`fg`).
   - Bersihkan ketidakkonsistenan tersembunyi di `Modal::Overwrite` ([src/app.rs](../src/app.rs#L1384)) yang menerima `'c'` walau tombol UI hanya menampilkan `Y Overwrite` dan `N Cancel`.
2. **Ergonomi `Field`:**
   - Tambah riwayat undo/redo internal ringan di `Field` (supaya `Ctrl+Z` saat salah tekan `Ctrl+U` tidak fatal).
   - Tambah `Ctrl+Backspace` / `Ctrl+Delete` untuk hapus per kata di `Field`.
   - Pada `App::path_input` ([src/app.rs](../src/app.rs#L776-L782)), pangkas trailing `\r`/`\n` saat paste satu baris dari clipboard agar menyalin path file tidak ditolak mentah-mentah, dan pisahkan pesan error karakter kontrol vs batas 4096 byte.

### 3.4. Explorer & Navigasi File (`src/explorer.rs`, `src/app.rs`, `src/ui.rs`)
1. **Visibilitas saat Root Gagal / >10.000 Entri:** Jika `workspace.explorer` bernilai `None` karena listing root gagal saat startup atau direktori melebihi 10.000 entri, jangan hilangkan panel sidebar secara diam-diam—tampilkan kartu error di Explorer dengan pesan penyebab dan tombol/hint `[F5] Retry` atau `[Bksp] Up`.
2. **Symlink & Issue Visibility:** Bedakan secara visual symlink yang mengarah ke direktori vs file vs *broken symlink* (tetap tanpa follow/traverse otomatis sesuai kebijakan keamanan di [SECURITY.md](SECURITY.md)), dan tampilkan isi `entry.issue` di status bar saat entri berlogo `" !"` dipilih.
3. **Navigasi cepat (non-IDE):** Tambah *type-ahead jump* (mengetik huruf saat fokus di Explorer melompat ke nama file terdekat) atau filter sederhana—sangat membantu di folder proyek besar tanpa mengubah Notefart menjadi IDE.
4. **Konsistensi Session Restore vs Explorer Visibility:** `App::bootstrap_session` ([src/app.rs](../src/app.rs#L256)) saat ini selalu memaksa `explorer_visible = true` menimpa `metadata.explorer_visible` yang baru saja dipulihkan oleh `Workspace::restore_session`. Jika memang desainnya selalu tampil saat startup, dokumentasikan alasannya di UI atau hormati state tersembunyi dari sesi terakhir.

### 3.5. Editor Teks & Manajemen Dokumen (`src/document.rs`, `src/app.rs`)
1. **Indentasi yang familiar:** Tambahkan `Shift+Tab` (`BackTab`) untuk mengurangi indentasi baris/blok terpilih (hingga 4 spasi) dan pertimbangkan block-indent saat `Tab` ditekan dengan seleksi multi-baris (saat ini `Tab` menghapus seluruh teks yang diseleksi!).
2. **Batas kata (`word_left` / `word_right`):** Di [src/document.rs](../src/document.rs#L306-L346), `\n` dihitung sebagai whitespace biasa sehingga `Ctrl+Right`/`Ctrl+Left` melompati beberapa baris kosong sekaligus. Pertimbangkan berhenti di batas baris / awal baris baru agar navigasi kata antar-paragraf lebih presisi.
3. **Menyalin/Menormalkan file `Mixed` Newline:** File dengan `Newline::Mixed` atau lone CR dibuka read-only ([src/file_io.rs](../src/file_io.rs#L341-L360)), tetapi `save_document_to` memblokir semua `read_only` termasuk **`Save As` (`F4`)**. Mengizinkan `Save As` untuk membuat salinan baru (dengan konfirmasi normalisasi ke LF/CRLF atau read-only guard khusus write-in-place) akan membuka jalan aman bagi pengguna.
4. **Pemberitahuan Highlight Parsial:** Untuk file besar/baris >16 KiB, tampilkan indikator ringkas persisten (mis. `[Plain]` atau ikon info di status/tab) saat highlight berhenti karena budget/error, alih-alih menimpa `App.status` secara acak dari idle loop.

---

## 4. Roadmap Saran Berdasarkan Dampak & Usaha

| Prioritas | Item Perbaikan | Dampak Utama |
| --- | --- | --- |
| **P0 — Segera** | Perbaiki `.fg(theme.accent)` di `draw_pane_card` & `.fg(theme.warning)` di `draw_sidebar`; tambah `fill_row` di compact `draw_header`. | Mencegah elemen fokus/warning tak terlihat di Light theme & belang background. |
| **P0 — Segera** | Perbaiki `Modal::Path` agar tidak hilang saat gagal Open/Save As; perbaiki *partial discard* pada multi-file `ClosePane`/`Quit`. | Mencegah frustrasi ketik ulang dan hilangnya dokumen dirty saat user menekan Cancel. |
| **P0 — Segera** | Selesaikan jebakan `discarding_string` pada `Alt+]`/`Alt+P`/`Alt+X`; lindungi pesan `20×6` di `Dialog::Choices` & notice keluar di `< 36` kolom. | Mencegah terminal terasa hang dan menjaga jalur keluar/peringatan tetap terbaca. |
| **P0 — Segera** | Perbaiki kontras `PALETTE_DARK_256` & `PALETTE_LIGHT_256` (`accent`/`warning` on `selection`) + lengkapi unit test; rapikan `StatusLevel` & cegah highlight menimpa status user. | Memenuhi target kontras 4.5:1 di semua pasangan UI aktual dan memastikan semua error tampil sebagai error. |
| **P1 — Menengah** | Atasi pemotongan helper 80 kolom (`Quit`, `Files`, `Pane`); nonaktifkan hit rect tombol overflow tab saat disabled; batasi `ScrollRight`; sinkronkan `shortcuts.rs`. | Pengalaman pengguna baru di terminal 80×24 jauh lebih jelas dan bebas anomali mouse. |
| **P1 — Menengah** | Aksesibilitas keyboard: `Alt+Shift` resize Explorer, `Tab`/panah pada tombol dialog, baris `..` & `Space` di Explorer, indikator target pane saat Explorer fokus, serta `[RO]` mini pada tab sempit. | Paritas keyboard-only yang jauh lebih kuat dan kejelasan fokus di layout split. |
| **P1 — Menengah** | Peningkatan Find/Replace: prefill seleksi, `"Match X of Y"` + `"Wrapped"`, avoid overlay menutupi match, serta konteks path/counter pada dialog konfirmasi. | Alur pencarian dan pengambilan keputusan save/overwrite jauh lebih percaya diri. |
| **P2 — Polish UX** | Scrollbar / indikator posisi scroll (Editor, Explorer, Help, Field overflow); toleransi double-click 1 sel + drag autoscroll di `Field` + hover state; `Shift+Tab` dedent & `Tab` block-indent; runtime switcher Theme/Icons. | Pengalaman harian mendekati editor desktop modern tanpa keluar dari kesederhanaan Notepad. |
