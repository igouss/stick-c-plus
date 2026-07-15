#!/usr/bin/env bb
;; Generate plant-display/src/sprite/generated.rs from gen/frames.json.
;;
;;   just sprites          # regenerate + verify
;;   bb plant-display/gen/generate.clj --check    # fail if the tree is stale
;;
;; frames.json is the vendored copy of the ClaudePix 20x20 creature animations
;; (https://claudepix.vercel.app/). See kb/sources/claudepix.md for how it was
;; obtained and why it is trustworthy: the frames were produced by executing the
;; site's own scripts, and each preset's sha256 was cross-checked against the same
;; frames re-derived inside a real browser. No JavaScript is needed here — the
;; frames are already explicit 20x20 grids of palette indices.
;;
;; Encoding, and the reasons for it:
;;   * 4 bits per cell. Every palette is <= 10 colours, so a nibble holds an index
;;     and a 20x20 frame packs into exactly 200 bytes of .rodata. 216 frames of the
;;     whole library is ~43 KB of flash; a firmware that names ONE sprite links only
;;     that one, because each is its own `static`.
;;   * Palette index 0 is transparent, in BOTH upstream families. Asserted, not
;;     assumed — a palette whose 0th entry is a real colour would silently paint a
;;     black box around every creature.
;;   * Colours are quantized RGB888 -> RGB565 here, at generation time, because that
;;     is the panel's pixel format and the quantization is lossy: doing it once, in a
;;     place a human can read the diff, beats doing it every frame on the device.

(require '[babashka.fs :as fs]
         '[babashka.process :refer [shell]]
         '[cheshire.core :as json]
         '[clojure.string :as str])

;; The frames each preset MUST hash to. Captured by re-deriving every preset inside a
;; real browser on the live site and hashing the result there — an independent path from
;; the one that produced frames.json. If upstream changes its art, or the vendored copy
;; is edited, this goes red and says so. Never relax it to make a build pass: a sprite
;; that silently became a different sprite is exactly the failure this pins down.
;;
;; sha256(JSON of [[hold_ms, grid], ...]), first 16 hex chars.
(def ^:private verified-digests
  {"idle_breathe"        "a9e05ad835b11414"
   "idle_blink"          "3ea6f3ee3af15424"
   "idle_look_around"    "790a1842935c4f8d"
   "expression_wink"     "55079a927c782dcd"
   "expression_surprise" "8f24eeadc967008f"
   "expression_sleep"    "f24e65ff987f4279"
   "dance_bounce"        "4ef0a019311a8635"
   "dance_sway"          "c24363a12fc9fe9a"
   "work_coding"         "77b17eaa7824b4c7"
   "work_think"          "cd4b4aea6f4062f6"
   "dance_bounce_dj"     "ca05217ee389c636"
   "dance_sway_dj"       "a8073b2a3387468a"
   "dance_djmix"         "f4d7167ea26a5e10"})

(defn- sha256-16
  "First 16 hex chars of the SHA-256 of `s`."
  [^String s]
  (->> (.getBytes s "UTF-8")
       (.digest (java.security.MessageDigest/getInstance "SHA-256"))
       (map #(format "%02x" (bit-and % 0xff)))
       (apply str)
       (take 16)
       (apply str)))

(defn- frames-digest
  "Hash a preset's frames the same way the browser did: [[hold, grid], ...] as JSON."
  [{:keys [frames]}]
  (sha256-16 (json/generate-string (mapv (fn [f] [(:hold_ms f) (:grid f)]) frames))))

(def ^:private root (-> *file* fs/parent fs/parent))          ; plant-display/
(def ^:private frames-json (fs/file root "gen" "frames.json"))
(def ^:private out-rs (fs/file root "src" "sprite" "generated.rs"))

(def ^:private grid 20)
;; Named separately because `validate!` destructures a frame's own `:grid` key, which
;; would otherwise shadow this.
(def ^:private grid-rows-count grid)
(def ^:private cells-per-frame (* grid grid))                 ; 400
(def ^:private packed-len (quot cells-per-frame 2))           ; 200
(def ^:private max-palette 16)                                ; one nibble

;; ---------------------------------------------------------------- colours

(defn- expand-hex
  "#abc -> #aabbcc. Upstream mixes 3- and 6-digit forms (dance_sway_dj uses '#555')."
  [s]
  (if (= 4 (count s))
    (str "#" (str/join (mapcat #(list % %) (subs s 1))))
    s))

(defn- rgb888
  "Parse a CSS hex colour to [r g b], or nil for `transparent`."
  [s]
  (when-not (= "transparent" s)
    (let [h (expand-hex s)]
      (when-not (re-matches #"#[0-9a-fA-F]{6}" h)
        (throw (ex-info "unparseable colour" {:colour s})))
      (mapv #(Integer/parseInt (subs h % (+ % 2)) 16) [1 3 5]))))

(defn- rgb565
  "Quantize [r g b] (8:8:8) to the panel's 5:6:5 channel depths."
  [[r g b]]
  [(bit-shift-right r 3) (bit-shift-right g 2) (bit-shift-right b 3)])

;; ---------------------------------------------------------------- packing

(defn- pack-frame
  "Row-major 20x20 indices -> 200 bytes, high nibble = even column."
  [grid-rows]
  (let [flat (vec (apply concat grid-rows))]
    (when (not= cells-per-frame (count flat))
      (throw (ex-info "frame is not 20x20" {:cells (count flat)})))
    (mapv (fn [i]
            (bit-or (bit-shift-left (nth flat (* 2 i)) 4)
                    (nth flat (inc (* 2 i)))))
          (range packed-len))))

(defn- byte-string
  "200 bytes as a Rust byte-string literal: b\"\\x00\\x11...\"."
  [bytes]
  (str "b\"" (str/join (map #(format "\\x%02x" %) bytes)) "\""))

;; ---------------------------------------------------------------- validation

(defn- validate!
  "Every invariant the Rust side will rely on. A malformed copy dies here, loudly,
   rather than painting garbage on a panel nobody is looking at."
  [{:keys [slug palette frames] :as preset}]
  (when (empty? frames)
    (throw (ex-info "preset has no frames" {:slug slug})))
  (when (> (count palette) max-palette)
    (throw (ex-info "palette exceeds one nibble" {:slug slug :colours (count palette)})))
  (when-not (= "transparent" (first palette))
    (throw (ex-info "palette index 0 is not transparent" {:slug slug :zeroth (first palette)})))
  (doseq [[i {:keys [hold_ms grid]}] (map-indexed vector frames)]
    (when-not (pos? hold_ms)
      (throw (ex-info "non-positive hold" {:slug slug :frame i :hold hold_ms})))
    (when (> hold_ms 65535)
      (throw (ex-info "hold exceeds u16" {:slug slug :frame i :hold hold_ms})))
    (when-not (= grid-rows-count (count grid))
      (throw (ex-info "frame is not 20 rows" {:slug slug :frame i :rows (count grid)})))
    (doseq [row grid]
      (when-not (= 20 (count row))
        (throw (ex-info "row is not 20 cells" {:slug slug :frame i :cells (count row)})))
      (doseq [v row]
        (when-not (< -1 v (count palette))
          (throw (ex-info "cell outside palette" {:slug slug :frame i :cell v}))))))
  preset)

;; ---------------------------------------------------------------- emit

(defn- const-name [slug] (str/upper-case (str/replace slug "-" "_")))

(defn- emit-palette [slug palette]
  (let [entries (map (fn [c]
                       (if-let [rgb (rgb888 c)]
                         (let [[r g b] (rgb565 rgb)]
                           (format "    Some(Rgb565::new(%d, %d, %d)), // %s" r g b c))
                         "    None, // transparent"))
                     palette)]
    (str (format "/// Palette of [`%s`]. Index 0 is transparent; the rest are the upstream\n" (const-name slug))
         "/// colours quantized to the panel's 5:6:5 channel depths.\n"
         (format "static P_%s: [Option<Rgb565>; %d] = [\n" (const-name slug) (count palette))
         (str/join "\n" entries)
         "\n];\n")))

(defn- emit-frames [slug frames]
  (str (format "static F_%s: [Frame; %d] = [\n" (const-name slug) (count frames))
       (str/join "\n"
                 (map (fn [{:keys [hold_ms grid]}]
                        (format "    Frame::new(%d, %s),"
                                hold_ms (byte-string (pack-frame grid))))
                      frames))
       "\n];\n"))

(defn- emit-sprite [{:keys [slug name category description family frames]}]
  (let [c (const-name slug)
        dur (reduce + (map :hold_ms frames))]
    (str (format "/// `%s` — %s\n///\n" name description)
         (format "/// %s family · %d frames · %.2f s loop.\n" family (count frames) (/ dur 1000.0))
         (format "pub static %s: Sprite = Sprite::new(\"%s\", \"%s\", &P_%s, &F_%s);\n" c slug category c c))))

(defn- emit [presets]
  (str "// @generated by plant-display/gen/generate.clj — DO NOT EDIT.\n"
       "//\n"
       "// Source: the ClaudePix 20x20 creature animation library,\n"
       "// <https://claudepix.vercel.app/>. Provenance, licence status and the\n"
       "// extraction recipe: kb/sources/claudepix.md.\n"
       "//\n"
       "// Regenerate with `just sprites`.\n"
       "#![allow(clippy::unreadable_literal)]\n\n"
       "use embedded_graphics::pixelcolor::Rgb565;\n\n"
       "use super::{Frame, Sprite};\n\n"
       (str/join "\n" (map (fn [p] (str (emit-palette (:slug p) (:palette p))
                                        "\n"
                                        (emit-frames (:slug p) (:frames p))
                                        "\n"
                                        (emit-sprite p)))
                           presets))
       "\n"
       "/// Every sprite in the library, in the upstream manifest's order.\n"
       "///\n"
       "/// Referencing this pulls all " (count presets) " into the image. A firmware that names a\n"
       "/// single sprite links only that one — each is its own `static`.\n"
       (format "pub static ALL: [&Sprite; %d] = [\n" (count presets))
       (str/join "\n" (map #(format "    &%s," (const-name (:slug %))) presets))
       "\n];\n"))

;; ---------------------------------------------------------------- formatting

(defn- rustfmt
  "Normalize Rust source through rustfmt, so the emitted file is byte-identical to what
   `cargo fmt --all` would leave behind. rustfmt reads stdin and writes stdout with
   `--emit stdout`; a non-zero exit means the generator emitted something unparseable,
   which must fail loudly rather than land as a broken file."
  [source]
  (let [{:keys [out exit err]} (shell {:in source :out :string :err :string :continue true}
                                      "rustfmt" "--edition" "2021" "--emit" "stdout")]
    (when-not (zero? exit)
      (throw (ex-info "rustfmt rejected the generated source" {:exit exit :stderr err})))
    ;; `--emit stdout` prefixes a `<stdin>:` banner line; drop it.
    (str/replace-first out #"^<stdin>:\n" "")))

;; ---------------------------------------------------------------- main

(defn -main [& args]
  (let [{:keys [presets]} (json/parse-string (slurp frames-json) true)
        _ (run! validate! presets)
        digests (into {} (map (juxt :slug frames-digest)) presets)]

    ;; Every preset must hash to what the live browser produced. Computed here, not read
    ;; from a field in frames.json — a corrupt copy would carry a matching field.
    (when-let [drift (seq (remove (fn [[slug d]] (= d (get verified-digests slug))) digests))]
      (throw (ex-info "frames do not match the browser-verified digests"
                      {:drift (mapv (fn [[slug d]]
                                      {:slug slug :got d :want (get verified-digests slug)})
                                    drift)})))
    (when-not (= (set (keys digests)) (set (keys verified-digests)))
      (throw (ex-info "preset set differs from the verified manifest"
                      {:got (sort (keys digests)) :want (sort (keys verified-digests))})))
    ;; Distinctness is implied by the digests above, but assert it directly: it is the
    ;; signature of the `window.PRESET` bleed that made one preset a copy of another.
    (when (not= (count digests) (count (set (vals digests))))
      (throw (ex-info "two presets have identical frames" {:digests digests})))

    ;; Emit through rustfmt, always. `cargo fmt --all` reformats generated.rs like any
    ;; other source, so an unformatted emit would make `--check` report STALE forever —
    ;; a permanent false red that trains everyone to ignore the gate.
    (let [rust (rustfmt (emit presets))
          frames (reduce + (map #(count (:frames %)) presets))]
      (if (some #{"--check"} args)
        (if (and (fs/exists? out-rs) (= rust (slurp out-rs)))
          (println "✓ sprites: generated.rs is in sync with frames.json")
          (do (println "✗ sprites: generated.rs is STALE — run `just sprites`")
              (System/exit 1)))
        (do (fs/create-dirs (fs/parent out-rs))
            (spit out-rs rust)
            (printf "%d sprites · %d frames · %d bytes packed → %s\n"
                    (count presets) frames (* packed-len frames)
                    (str (fs/relativize (fs/cwd) out-rs))))))))

(when (= *file* (System/getProperty "babashka.file")) (apply -main *command-line-args*))
