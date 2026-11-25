# 0020: Non-image uploads

- Status: Proposed (scope accepted 2026-11-19; the design below is for review)
- Date: 2026-11-19

## Context

Uploads of images were ported in MVP 6. Every other type of PHP's default allowed list (`get_allowed_mime_types()`) answered 501, because `wp_check_filetype_and_ext()` asks PHP's `fileinfo` extension (libmagic) what a file really is, and `wp_generate_attachment_metadata()` reads audio and video with getID3 and makes a PDF's preview images with Imagick over Ghostscript. Decision (2026-11-19): every type PHP's default list takes is ported. HyScribe may use no C library and no child process, and every parser of untrusted bytes is bounded, memory safe and fuzzed (ADR 0006).

## Decision

### The type check

`wp_check_filetype_and_ext()` is ported whole: the extension's type, the image branch, and the vetting by `fileinfo` that follows (the nonspecific types, the `audio/` and `video/` leeway, the text, CSV and RTF special cases, the exact match for the rest). `hyscribe-image`'s `fileinfo` module stands for `finfo_file( FILEINFO_MIME_TYPE )`: a bounded reader of the first bytes (and of a ZIP's central directory) that gives every type libmagic may answer for a file, or says it cannot tell.

- The check decides only where every type libmagic may answer leads to the same verdict. Where they differ (a Word file named `.zip`, whose type libmagic names by parts of its own choosing) or the reader does not know the file (Word 97 and other Compound File Binary files, unknown binary), the upload answers 501, as before
- Text is `text/plain` or `text/csv` by libmagic's own comma counting (`csv_parse()` is ported); text with a marker of source code, markup, mail, a patch or a configuration file is left, as libmagic has a type for each
- Recognised: PDF, plain text, CSV, WebVTT, iCalendar, RTF, ZIP (OpenDocument and EPUB by the stored `mimetype`, Office Open XML by the folders of its parts, any other archive as ZIP), TAR, gzip, 7z, RAR, Java classes, and the audio and video containers (MP3 by its frame header or ID3v2 tag, WAVE, AVI, Ogg, FLAC, MIDI, MP4 and QuickTime by their brand, 3GPP, ASF, Matroska and WebM by the `DocType`, FLV, MPEG, ADTS)
- Matched to libmagic 5.46 (the oracle's image) by `reference/fileinfo/golden.sh`: every corpus file, synthetic headers of each family, mutated and cut copies of them, ZIP layouts, and prose of many shapes. Where the reader answers, libmagic's answer is among its types: the test fails otherwise. Fuzz target `upload_fileinfo`

### The attachment

- Documents, archives, text and every type libmagic and PHP agree on get the attachment PHP makes: the rows of `media_handle_upload()` and the REST insert, and `_wp_attachment_metadata` holding `filesize` (matched row for row to the write oracle)
- A PDF answers 501 after its type check passes. PHP makes `-pdf.jpg` preview images of it with Imagick over Ghostscript, whose pixels no Rust code reproduces (2026-11-19: keep 501 for a preview that cannot be matched)
- Audio and video: `wp_read_audio_metadata()` and `wp_read_video_metadata()` read through getID3, which WordPress ships without most format modules. For a format whose module WordPress removed (MIDI, AAC in ADTS or ADIF, MPEG video) getID3 names the type and stops, so the metadata is `filesize` and `mime_type`; HyScribe ports that, and WAVE of uncompressed PCM (`hyscribe-image`'s `getid3`, matched to PHP by `reference/getid3/golden.sh`). A file with an ID3, APE or Lyrics3 tag, or of fewer than 512 bytes (below which getID3 stops early), is left. MP3, MP4 and QuickTime, Ogg, FLAC, ASF, Matroska, AVI and FLV, whose getID3 modules read more than the header, answer 501 until their readers are ported
- The hooks `wp_read_audio_metadata` and `wp_read_video_metadata` are exposed under their names with the metadata and the file format; getID3's raw `$data` is not given

### Serving

Unchanged from MVP 6: uploads are served as static files with `nosniff`; images (not SVG), audio, video, PDFs and plain text are shown with their type, and every other type, HTML and SVG among them, is sent as an `attachment` download.

### Amendment, 2026-11-24: Word, Excel and PowerPoint 97 to 2003 files

Decision (2026-11-24): the old Office files upload, which replaces the 501 the consequences below name for them. `hyscribe-image`'s `fileinfo` reads Compound File Binary files (`.doc`, `.xls`, `.ppt` and the other types of WordPress's list stored in one, such as `.pps`, `.pot`, `.xlt` and `.mpp`) the way libmagic's `file_trycdf()` types them, so `wp_check_filetype_and_ext()` decides as PHP does, and the attachment is that of any other document (the rows of `media_handle_upload()` and the REST insert, `_wp_attachment_metadata` holding `filesize`). Nothing is written that PHP would not write (ADR 0003).

- libmagic types such a file by the application name of the summary information (`Word`, `Excel`, `PowerPoint` and a few installer names, in any case, anywhere in the name), else by a word in a directory entry's name (`WordDocument`, `Book`, `PowerPoint`), else as `application/vnd.ms-office`; a file with no summary information by the streams it holds, else as `application/x-ole-storage`. The reader repeats that order from libmagic's source (5.45, the one PHP's `fileinfo` carries) and is matched to it by about 670 Compound File Binary samples of `reference/fileinfo/samples.py`
- It answers only where libmagic's reading cannot go another way: 512 byte sectors, a short stream cut-off of 4096, no table sectors beyond the header's 109, chains that end, and summary properties of the types libmagic prints. A damaged or cut file, a name with no end, a second application name, and a file libmagic's tar or SIMH tape tests (which run first) may take answer 501. It never takes a file PHP refuses
- The reader is bounded: every chain stops at libmagic's 10,000 sectors, every read is a slice of the upload, nothing is allocated beyond the sectors a chain really holds, and the time is linear in the file. The fuzz target `upload_fileinfo` also alters a real file
### Amendment, 2026-11-24: PDFs without previews

Decision: a PDF uploads without `-pdf.jpg` preview images, exactly as PHP WordPress writes it on a host without Imagick and Ghostscript, which replaces the 501 the attachment section names for it. With no image editor, `wp_generate_attachment_metadata()` makes no preview and the attachment's `_wp_attachment_metadata` holds `filesize` alone; HyScribe writes those same rows and nothing PHP would not write (ADR 0003), and needs no PDF renderer.

- The upload goldens record PHP with the must-use plugin `reference/uploads/no-pdf-preview.php`, which leaves no image editor while a PDF's metadata is made (images keep theirs): the upload routes and the write oracle's `upload-async-pdf` and `upload-rest-raw-pdf` match row for row
- The media library, the attachment edit screen and REST `media_details` of such a PDF (an icon, no sizes) were already matched to PHP over attachments of the same metadata, and are unchanged
- A site moving to PHP WordPress with Imagick keeps the PDFs without previews until they are regenerated, as on any host that gains Imagick

## Consequences

- Plain documents, spreadsheets, presentations (Office Open XML and OpenDocument), archives, CSV, subtitles and calendars upload as in PHP, with the same refusals for a file that is not what its name says
- Word 97 to 2003 files (`.doc`, `.xls`, `.ppt`), PDFs, and the audio and video getID3 reads in depth (MP3, MP4 among them) answer 501 until their readers are ported; each needs its own differential corpus
- The libmagic reader is a hand written subset: it can answer 501 for a file PHP takes, never take a file PHP refuses for a family it reads, and the golden test is the guard. A new family comes with samples in `reference/fileinfo/samples.py`

## Amendment: MP3 (2026-11-24)

Decision: MP3 and MP4 files must upload. MP3 is ported first. `hyscribe-image`'s `getid3` module reads an MP3 file as getID3's `audio.mp3` module and its ID3 modules do, for the fields `wp_read_audio_metadata()` keeps, and `reference/getid3/golden.sh` matches it to PHP file by file over the upload corpus and the files `reference/getid3/mp3-samples.py` builds.

### Licence

getID3 (`wp-includes/ID3/`, `license.txt`) is offered by James Heinrich under several licences at the user's choice: GNU GPL versions 3, 2 and 1, GNU LGPL version 3 and Mozilla MPL 2.0. The port is taken under the GNU GPL version 3, which section 13 of the AGPL allows to be combined with AGPL code, as ADR 0004 reads WordPress's own licence. Each ported file names getID3 and its author beside the SPDX line.

### What is read

- Layers 1, 2 and 3 of MPEG 1, 2 and 2.5: the frame header, the length by getID3's own arithmetic (PHP's integer and float results kept apart), the Xing, Info and VBRI headers, the LAME tag (version, method, preset, lowpass, the encoder name completed from the end of the file, `GuessEncoderOptions()`), the bitrate mode and `compression_ratio`
- ID3v2.3 and 2.4 (unsynchronisation, the extended header, per frame unsynchronisation and data length) with the comments `wp_add_id3_tag_data()` copies from the text frames (`T???`, `TXXX`) and comments (`COMM`), getID3's genre, track, total tracks and year rules, the encodings ISO-8859-1, UTF-8 and well formed UTF-16; ID3v1 and ID3v1.1; both together, ID3v2 first; each value through `wp_kses_post()`
- The title and text of an audio attachment from its tags, as `media_handle_upload()` writes them, after the filter `wp_read_audio_metadata`
- The first `APIC` picture that is a GIF, JPEG or PNG: as `wp_generate_attachment_metadata()` does, it is written by `wp_upload_bits()` into the folder of the month as `<file>-mp3-image.<ext>`, made an attachment of its own with its sizes and `_cover_hash`, or the attachment of the same picture made before is found by that hash, and it becomes the audio's `_thumbnail_id`; the metadata keeps the picture's type and size. The hooks `wp_upload_bits` and `attachment_thumbnail_args` run. The upload routes and the write oracle match PHP's answers and rows (`reference/upload-routes/golden.sh`, `upload-*-mp3*` of `reference/writes/actions-uploads.php`)

### What still answers 501

Where getID3 would warn, rescan or read the file another way, so the metadata could not be matched with certainty:

- Garbage before the first frame, a second frame that does not follow, a variable bitrate stream without a Xing, Info or VBRI header (getID3 reads it by histogram), free format, a Xing or VBRI header in a later frame, layer 2 combinations getID3 refuses, a file of its Xing or Info frame alone (whose length PHP gives in a way not matched), files below 512 bytes
- APE and Lyrics3 tags, ID3v2.2 and ID3v2.5, ID3v2 footers, compressed, encrypted or grouped frames, the `restrictions` flag of an ID3v2.4 extended header (which stops PHP with a fatal error), more than eight extended flag bytes, URL frames (`W???`), `IPLS`, `MCDI`, `USLT`, `LINK` and `USER`, an invalid frame name or size, an invalid encoding byte, UTF-16 that mbstring would repair, a numeric genre outside the table
- An embedded picture that is not a GIF, JPEG or PNG getID3 reads the size of (PHP stores it without a type or size)
- A track number whose `number_format()` would pass a float's exact integers
- MP4, QuickTime and the rest of the list above are unchanged

## Amendment: MP4 and QuickTime (2026-11-25)

The second half of the MP3 and MP4 decision. `hyscribe-image`'s `getid3` reads a file getID3 hands to its `audio-video.quicktime` module (MP4, M4A, M4V, QuickTime, 3GPP and 3GPP2) as that module and `getID3::analyze()` do, for the fields `wp_read_video_metadata()` and `wp_read_audio_metadata()` keep. The port is taken under the GNU GPL version 3, as the MP3 port is (licence section above).

### What is read

- The atoms top down as getID3 walks them, its quirks kept: a top level atom is read eight bytes past its end (so a list holding the next atom's header reads it as a last child), a list skips four zero bytes, an atom of a size below its header's takes the rest of the list less its last bytes, a 64 bit size is read, a size of 0 stops the walk, and an atom past the end of the file stops the module before its own sums
- The movie header's length, the track header's size (largest enabled track), the time scale and the frame rate of the time to sample table as getID3 keeps them, the sample descriptions (MP4's video formats and their codec names, a vendor's QuickTime description with its compressor's name, AAC's channels, sample rate and bits, empty descriptions read past a table's end), the elementary stream descriptor's average bitrate, where the media data starts, the QuickTime VR markers
- `analyze()`'s sums after the module: the bitrate from the media data and the length, the video's and the audio's share of it, the channel mode, the compression ratio and `CleanUp()`; the MP4 and audio-only switches of `fileformat` and `mime_type`
- `created_timestamp` from the first atom of the last top level `moov` (`wp_get_media_creation_timestamp()`), including the value before 1970 an encoder's zero date gives
- The first cover picture of an `ilst` item `covr`: as for MP3, an attachment of its own written by `wp_upload_bits()`, found again by `_cover_hash`, the audio or video's `_thumbnail_id`; the metadata keeps its type only, as `wp_add_id3_tag_data()` gives it no size
- A video attachment's content starts with the player `prepend_attachment()` makes (`wp_video_shortcode()` with the size of its metadata and its cover picture as the poster), so the REST answer to a video upload, which renders it, is PHP's
- Matched to PHP by `reference/getid3/golden.sh` over the corpus (ffmpeg files of every kind above, `reference/uploads/media-corpus.sh`) and the files `reference/getid3/mp4-samples.py` builds, by the upload routes' golden (`async-upload.php`, Add New Media and `wp/v2/media`) and by the write oracle's `upload-*-mp4`, `-m4a-cover`, `-mov` and `-m4v` actions

### What answers 501

Where the result would depend on PHP's memory limit, reach another getID3 module, or follow a path not ported:

- Memory: a file whose atoms besides media data hold over 16 MiB, nest deeper than 32 or number over 20,000, a time table of more than 13,421 entries (the limit under 128M), more than 64 empty sample descriptions; where getID3's read buffer of a quarter of the memory limit cuts the media data (32, 64, 128 or 256 MiB in) or where its read stops, an MPEG audio frame sync (getID3 would read the file as MP3 in MP4)
- Formats: a first atom of 32 MiB or more (its first byte could make an earlier getID3 format match), the patterns of MPEG, S3M and MOD files, HEIF brands, compressed (`cmov`), fragmented (`moof` and its boxes) and image item movies, Nikon and Kodak pictures, `gps `, `ID32`, metadata `keys` and numbered item names (iPhone files), 360fly `uuid` data, track reference lists (`tmcd`, `chap` and the like), a table claiming more entries than it holds (`dref`, `elst`, `iods`), media data holding chapter titles, media data inside another atom
- Shapes not ported: a top level atom smaller than its header, a movie or media header with a time scale of 0 or cut short, a track header cut short, a sample description cut short or of a size below 16, a cover data box of a size below 8, a frame rate of exactly half a thousandth
- A file below 512 bytes or with an ID3v1, APE or Lyrics3 tag at its end, as for the other formats
- A first cover picture that is not a GIF, JPEG or PNG
- Ogg, FLAC, ASF, Matroska, AVI and FLV are unchanged

### Security

The reader takes slices of the upload only, with no `unsafe`, no indexing that can fail and sizes compared before any slice; nesting, atoms and bytes held have the fixed limits above, so time and memory are linear in the file. Fuzz target `upload_quicktime`, seeded with the samples in CI.
