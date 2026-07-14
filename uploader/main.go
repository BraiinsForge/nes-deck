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
	installedROMRoot        = "/mnt/data/roms"
	installedBaseCatalog    = "/mnt/data/nes-deck/menu/games.tsv"
	installedUploadCatalog  = "/mnt/data/nes-deck/uploads/games.tsv"
)

func usage() {
	fmt.Fprintf(os.Stderr, "Usage:\n  %s\n  %s --init-password PATH\n  %s --set-password PATH\n", os.Args[0], os.Args[0], os.Args[0])
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

func runServer() error {
	password, err := loadPasswordConfig(installedPasswordConfig)
	if err != nil {
		return err
	}
	store := &romStore{
		romRoot:        installedROMRoot,
		baseCatalog:    installedBaseCatalog,
		uploadCatalog:  installedUploadCatalog,
		restartCatalog: restartDashboard,
	}
	app, err := newApplication(password, store, true)
	if err != nil {
		return err
	}
	listener, err := listenWireGuard()
	if err != nil {
		return fmt.Errorf("listen on %s through %s: %w", serviceAddress, serviceInterface, err)
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
	log.Printf("ROM uploader listening at %s on %s only", serviceAddress, serviceInterface)
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
	case len(os.Args) == 3 && os.Args[1] == "--init-password":
		_, err = initializePassword(os.Args[2], os.Stdout)
	case len(os.Args) == 3 && os.Args[1] == "--set-password":
		err = setPassword(os.Args[2])
	default:
		usage()
		os.Exit(2)
	}
	if err != nil {
		log.Printf("rom-uploader: %v", err)
		os.Exit(1)
	}
}
