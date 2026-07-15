#!/usr/bin/env bb
;; Re-run the sprite fill-throughput measurement (see README.md).
;;
;;   bb kb/experiments/2026-07-09-sprite-fill-throughput/run.clj [seconds]
;;
;; Flashes the plant-monitor, streams serial for `seconds` (default 45), and counts the
;; render loop's over-budget warnings. Requires the board on /dev/ttyUSB0.
;;
;; THE PROBE MUST BE DISCONNECTED OR DRY. Only an unhealthy observation draws an
;; ANIMATED creature (50 ms budget); a healthy one is motionless, never repaints, and
;; would report zero warnings while measuring nothing at all. That is the false green
;; this script exists to refuse, so it counts the animated cycles too and calls the run
;; INCONCLUSIVE when there were none.

(require '[babashka.process :as p]
         '[clojure.string :as str])

(def seconds (parse-long (or (first *command-line-args*) "45")))
(def repo (-> *file* java.io.File. .getParentFile .getParentFile .getParentFile .getParentFile))
(def log (java.io.File/createTempFile "sprite-throughput" ".log"))

(defn- count-matching [lines re] (count (filter #(re-find re %) lines)))

(println (format "▶ flashing + streaming %ds of serial (probe must be disconnected/dry)" seconds))
(let [proc (p/process {:dir (str repo) :out :write :out-file log :err :write :err-file log}
                      "just" "run")]
  (Thread/sleep (* 1000 (+ seconds 25)))     ; +25s covers build, flash and boot
  (p/destroy-tree proc))

(let [lines      (str/split-lines (slurp log))
      over       (count-matching lines #"over the .* tick budget")
      failures   (count-matching lines #"render failed")
      crashes    (count-matching lines #"(?i)panic|task_wdt|Guru Meditation")
      boots      (count-matching lines #"std/ESP-IDF up")
      animated   (count-matching lines #"probe faulted")
      healthy    (count-matching lines #"serving moisture")]

  (println)
  (doseq [[k v] [["over-budget paints" over] ["render failures" failures]
                 ["panics / watchdog" crashes] ["boots" boots]
                 ["faulted (animated) cycles" animated] ["fresh (still) cycles" healthy]]]
    (println (format "  %-26s %d" k v)))
  (println (format "\n  full log: %s" log))
  (println)

  (cond
    (zero? boots)
    (do (println "❌ INCONCLUSIVE: the board never booted — check /dev/ttyUSB0 and the build.")
        (System/exit 2))

    (zero? animated)
    (do (println "❌ INCONCLUSIVE: no animated cycles. The probe read healthy, so the creature")
        (println "   never moved and no paint was ever measured. Zero warnings here means")
        (println "   NOTHING. Disconnect or dry the probe and run again.")
        (System/exit 2))

    (pos? (+ over failures crashes))
    (do (println (format "❌ FAIL: %d over-budget paint(s), %d render failure(s), %d crash(es)."
                         over failures crashes))
        (println "   A frame no longer fits its tick. See README.md — the last time this")
        (println "   happened, `draw_onto` was setting 400 address windows per frame.")
        (System/exit 1))

    :else
    (println (format "✅ PASS: %d animated cycles, every paint inside its 50 ms tick." animated))))
