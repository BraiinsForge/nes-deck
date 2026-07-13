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
   :color "#E48A75")
  (:id "falling"
   :title "FALLING"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/falling.nes"
   :color "#9691D6")
  (:id "thwaite"
   :title "THWAITE"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/thwaite.nes"
   :color "#E7B676")
  (:id "concentration-room"
   :title "CONCENTRATION ROOM"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/croom.nes"
   :color "#82B7AF")
  (:id "robotfindskitten"
   :title "ROBOTFINDSKITTEN"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/robotfindskitten.nes"
   :color "#9CC690")
  (:id "micro-mages"
   :title "MICRO MAGES"
   :system :nes
   :rom "/mnt/data/nes-deck/roms/micro-mages.nes"
   :color "#DF8FA7")
  (:id "adjustris"
   :title "ADJUSTRIS"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/adjustris.gb"
   :color "#ABB96E")
  (:id "pokemon-red"
   :title "POKEMON RED"
   :system :gb
   :rom "/mnt/data/nes-deck/roms/pokemon-red.gb"
   :color "#E48A75")
  (:id "geometrix"
   :title "GEOMETRIX"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/geometrix.gbc"
   :color "#84AFCC")
  (:id "donkey-kong-country"
   :title "DONKEY KONG COUNTRY"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/donkey-kong-country.gbc"
   :color "#E7B676")
  (:id "super-mario-bros-deluxe"
   :title "SUPER MARIO BROS. DELUXE"
   :system :gbc
   :rom "/mnt/data/nes-deck/roms/super-mario-bros-deluxe.gbc"
   :color "#E48A75")
  (:id "outlaw"
   :title "OUTLAW"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/outlaw.ch8"
   :color "#BE926F")
  (:id "space-racer"
   :title "SPACE RACER"
   :system :chip8
   :rom "/mnt/data/nes-deck/roms/spaceracer.ch8"
   :color "#EAE4DC")
  (:id "ten-seconds"
   :title "10 SECONDS"
   :system :deck
   :rom "/mnt/data/nes-deck/games/ten-seconds"
   :color "#F09C77")))
