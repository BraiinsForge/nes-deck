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
   :color "#D78787")
  (:id "falling"
   :title "FALLING"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/falling.nes"
   :color "#8787D7")
  (:id "thwaite"
   :title "THWAITE"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/thwaite.nes"
   :color "#D7AF87")
  (:id "concentration-room"
   :title "CONCENTRATION ROOM"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/croom.nes"
   :color "#87AFAF")
  (:id "robotfindskitten"
   :title "ROBOTFINDSKITTEN"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/robotfindskitten.nes"
   :color "#AFD787")
  (:id "micro-mages"
   :title "MICRO MAGES"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/micro-mages.nes"
   :color "#D787AF")
  (:id "adjustris"
   :title "ADJUSTRIS"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/adjustris.gb"
   :color "#AFAF5F")
  (:id "pokemon-red"
   :title "POKEMON RED"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/pokemon-red.gb"
   :color "#D78787")
  (:id "geometrix"
   :title "GEOMETRIX"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/geometrix.gbc"
   :color "#87AFD7")
  (:id "donkey-kong-country"
   :title "DONKEY KONG COUNTRY"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/donkey-kong-country.gbc"
   :color "#D7AF87")
  (:id "super-mario-bros-deluxe"
   :title "SUPER MARIO BROS. DELUXE"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/super-mario-bros-deluxe.gbc"
   :color "#D78787")
  (:id "outlaw"
   :title "OUTLAW"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/outlaw.ch8"
   :color "#AF875F")
  (:id "space-racer"
   :title "SPACE RACER"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/spaceracer.ch8"
   :color "#E4E4E4")
  (:id "ten-seconds"
   :title "10 SECONDS"
   :system :deck
   :rom "/mnt/data/nes-deck/games/ten-seconds"
   :color "#FFAF87")))
