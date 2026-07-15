;; Deck menu catalog and dashboard palette, schema version 4.
;;
;; Each game has exactly five fields.  The compiler writes them to games.tsv
;; in this order: id, title, system, rom, color.
(:version 4
 :palette
  (:background 16
   :text-dark 233
   :field 233
   :surface 234
   :inactive-border 59
   :control-border 242
   :footer 250
   :inactive-text 253
   :text 255
   :white 231
   :title 229
   :volume-off 138
   :volume-on 108
   :selected 109
   :wifi-active 67
   :wifi-focus 111
   :wifi-active-border 147
   :field-label 145
   :accent 202
   :active 237
   :control-surface 236
   :muted 246)
 :games
  ((:id "mario"
   :title "SUPER MARIO BROS."
   :system :nes
   :rom "/mnt/data/roms/nes/super-mario-bros.nes"
   :color "#D78787")
  (:id "micro-mages"
   :title "MICRO MAGES"
   :system :nes
   :rom "/mnt/data/roms/nes/micro-mages.nes"
   :color "#D787AF")
  (:id "kirbys-adventure"
   :title "KIRBY'S ADVENTURE"
   :system :nes
   :rom "/mnt/data/roms/nes/kirbys-adventure.nes"
   :color "#D787AF")
  (:id "metroid"
   :title "METROID"
   :system :nes
   :rom "/mnt/data/roms/nes/metroid.nes"
   :color "#8787D7")
  (:id "tetris"
   :title "TETRIS"
   :system :nes
   :rom "/mnt/data/roms/nes/tetris.nes"
   :color "#87AFAF")
  (:id "pokemon-red"
   :title "POKEMON RED"
   :system :gb
   :rom "/mnt/data/roms/gb/pokemon-red.gb"
   :color "#D78787")
  (:id "final-fantasy-legend-iii"
   :title "FINAL FANTASY LEGEND III"
   :system :gb
   :rom "/mnt/data/roms/gb/final-fantasy-legend-iii.gb"
   :color "#D7D787")
  (:id "kirbys-dream-land"
   :title "KIRBY'S DREAM LAND"
   :system :gb
   :rom "/mnt/data/roms/gb/kirbys-dream-land.gb"
   :color "#AFAFAF")
  (:id "donkey-kong-country"
   :title "DONKEY KONG COUNTRY"
   :system :gbc
   :rom "/mnt/data/roms/gbc/donkey-kong-country.gbc"
   :color "#D7AF87")
  (:id "super-mario-bros-deluxe"
   :title "SUPER MARIO BROS. DELUXE"
   :system :gbc
   :rom "/mnt/data/roms/gbc/super-mario-bros-deluxe.gbc"
   :color "#D78787")
  (:id "elite"
   :title "ELITE"
   :system :zx
   :rom "/mnt/data/roms/zx/elite-joystick-club-version.tap"
   :color "#87AFFF")
  (:id "knight-lore"
   :title "KNIGHT LORE"
   :system :zx
   :rom "/mnt/data/roms/zx/knight-lore.tap"
   :color "#D7AF5F")
  (:id "outlaw"
   :title "OUTLAW"
   :system :chip8
   :rom "/mnt/data/roms/chip8/outlaw.ch8"
   :color "#AF875F")
  (:id "space-racer"
   :title "SPACE RACER"
   :system :chip8
   :rom "/mnt/data/roms/chip8/spaceracer.ch8"
   :color "#E4E4E4")
  (:id "ten-seconds"
   :title "10 SECONDS"
   :system :deck
   :rom "/mnt/data/nes-deck/games/ten-seconds"
   :color "#FFAF87")))
