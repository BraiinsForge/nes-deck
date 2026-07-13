;; Deck menu catalog, schema version 3.
;;
;; Each game has exactly five fields.  The compiler writes them to games.tsv
;; in this order: id, title, system, rom, color.
(:version 3
 :games
  ((:id "mario"
   :title "SUPER MARIO BROS."
   :system :nes
   :rom "/mnt/data/nes-deck/mario.nes"
   :color "#E74C3C")
  (:id "falling"
   :title "FALLING"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/falling.nes"
   :color "#6C5CE7")
  (:id "thwaite"
   :title "THWAITE"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/thwaite.nes"
   :color "#F5A623")
  (:id "concentration-room"
   :title "CONCENTRATION ROOM"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/croom.nes"
   :color "#00A8A8")
  (:id "robotfindskitten"
   :title "ROBOTFINDSKITTEN"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/robotfindskitten.nes"
   :color "#62C370")
  (:id "micro-mages"
   :title "MICRO MAGES"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/micro-mages.nes"
   :color "#E05B9A")
  (:id "adjustris"
   :title "ADJUSTRIS"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/adjustris.gb"
   :color "#8BAC0F")
  (:id "pokemon-red"
   :title "POKEMON RED"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/pokemon-red.gb"
   :color "#E74C3C")
  (:id "geometrix"
   :title "GEOMETRIX"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/geometrix.gbc"
   :color "#2D98DA")
  (:id "donkey-kong-country"
   :title "DONKEY KONG COUNTRY"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/donkey-kong-country.gbc"
   :color "#F5A623")
  (:id "super-mario-bros-deluxe"
   :title "SUPER MARIO BROS. DELUXE"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/super-mario-bros-deluxe.gbc"
   :color "#E74C3C")
  (:id "outlaw"
   :title "OUTLAW"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/outlaw.ch8"
   :color "#AA6633")
  (:id "space-racer"
   :title "SPACE RACER"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/spaceracer.ch8"
   :color "#FCFCFC")))
