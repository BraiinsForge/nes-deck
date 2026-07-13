;; Deck menu catalog, schema version 1.
;;
;; Each game has exactly six fields.  The compiler writes them to games.tsv
;; in this order: id, title, rom, description, color, license.
(:version 1
 :games
 ((:id "mario"
   :title "SUPER MARIO BROS."
   :rom "/mnt/data/nes-deck/mario.nes"
   :description "Press Start to begin; Mario's title and attract demo are silent."
   :color "#E74C3C"
   :license "Proprietary; user-supplied, not redistributed")
  (:id "falling"
   :title "FALLING"
   :rom "/mnt/data/nes-deck/roms/falling.nes"
   :description "Survive a fast arcade challenge as the difficulty keeps rising."
   :color "#6C5CE7"
   :license "MIT")
  (:id "thwaite"
   :title "THWAITE"
   :rom "/mnt/data/nes-deck/roms/thwaite.nes"
   :description "Defend your town from missiles with firework anti-ballistic missiles."
   :color "#F5A623"
   :license "GPL-3.0-or-later")
  (:id "concentration-room"
   :title "CONCENTRATION ROOM"
   :rom "/mnt/data/nes-deck/roms/croom.nes"
   :description "Match cards, clear the toxin, and face CPU or human rivals."
   :color "#00A8A8"
   :license "GPL-3.0-or-later; exact-ROM copy exception")
  (:id "robotfindskitten"
   :title "ROBOTFINDSKITTEN"
   :rom "/mnt/data/nes-deck/roms/robotfindskitten.nes"
   :description "Guide robot through non-kittens until robot finds kitten."
   :color "#62C370"
   :license "zlib License")))
