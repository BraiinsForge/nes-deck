package main

import (
	"errors"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

const (
	installedPasswordConfig = "/mnt/data/nes-deck/uploader/password.conf"
	installedAddressConfig  = "/mnt/data/nes-deck/uploader/address.conf"
	installedROMRoot        = "/mnt/data/roms"
	installedBaseCatalog    = "/mnt/data/nes-deck/menu/games.tsv"
	installedUploadCatalog  = "/mnt/data/nes-deck/uploads/games.tsv"
	installedActivePalette  = "/mnt/data/nes-deck/state/palette.tsv"
	installedBasePalette    = "/mnt/data/nes-deck/menu/palette.tsv"
	installedPaletteConfig  = "/mnt/data/nes-deck/state/dashboard-palette.sexp"
)

func usage() {
	fmt.Fprintf(os.Stderr, "Usage:\n  %s\n  %s --set-password PATH\n  %s --check-password-config PATH\n  %s --check-address PATH\n", os.Args[0], os.Args[0], os.Args[0], os.Args[0])
}

func setPassword(path string) error {
	password, err := readPassword(os.Stdin)
	if err != nil {
		return err
	}
	config, err := newPasswordConfig(password)
	if err != nil {
		return err
	}
	return atomicWrite(path, encodePasswordConfig(config), 0600)
}

func loadServiceAddress(path string) (string, error) {
	contents, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("read service address: %w", err)
	}
	if len(contents) == 0 || len(contents) > 64 || contents[len(contents)-1] != '\n' {
		return "", errors.New("service address configuration has an invalid size")
	}
	return normalizeServiceAddress(string(contents[:len(contents)-1]))
}

func runServer() error {
	password, err := loadPasswordConfig(installedPasswordConfig)
	if err != nil {
		return err
	}
	address, err := loadServiceAddress(installedAddressConfig)
	if err != nil {
		return err
	}
	store := &romStore{
		romRoot:        installedROMRoot,
		baseCatalog:    installedBaseCatalog,
		uploadCatalog:  installedUploadCatalog,
		restartCatalog: restartDashboard,
	}
	palette := &paletteStore{
		activePath:       installedActivePalette,
		fallbackPath:     installedBasePalette,
		overridePath:     installedPaletteConfig,
		restartDashboard: restartDashboard,
	}
	app, err := newApplication(password, store, palette, true, address)
	if err != nil {
		return err
	}
	listener, err := listenWireGuard(address)
	if err != nil {
		return fmt.Errorf("listen on %s through %s: %w", address, serviceInterface, err)
	}
	server := &http.Server{
		Handler:           app,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       35 * time.Second,
		WriteTimeout:      35 * time.Second,
		IdleTimeout:       30 * time.Second,
		MaxHeaderBytes:    16 * 1024,
	}
	stopping := make(chan os.Signal, 1)
	signal.Notify(stopping, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-stopping
		_ = server.Close()
	}()
	log.Printf("ROM uploader listening at %s on %s only", address, serviceInterface)
	err = server.Serve(listener)
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}

func main() {
	log.SetFlags(log.Ldate | log.Ltime | log.LUTC)
	var err error
	switch {
	case len(os.Args) == 1:
		err = runServer()
	case len(os.Args) == 3 && os.Args[1] == "--set-password":
		err = setPassword(os.Args[2])
	case len(os.Args) == 3 && os.Args[1] == "--check-password-config":
		_, err = loadPasswordConfig(os.Args[2])
	case len(os.Args) == 3 && os.Args[1] == "--check-address":
		_, err = loadServiceAddress(os.Args[2])
	default:
		usage()
		os.Exit(2)
	}
	if err != nil {
		log.Printf("rom-uploader: %v", err)
		os.Exit(1)
	}
}
