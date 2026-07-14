package main

import (
	"archive/zip"
	"bufio"
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"unicode"
	"unicode/utf8"
)

const (
	maximumRequestBytes = 12 * 1024 * 1024
	maximumArchiveBytes = 10 * 1024 * 1024
	maximumCatalogBytes = 64 * 1024
	maximumGames        = 64
)

type systemSpec struct {
	extension string
	color     string
	maximum   int
}

var supportedSystems = map[string]systemSpec{
	"nes":   {extension: ".nes", color: "#FF5F00", maximum: 8 * 1024 * 1024},
	"gb":    {extension: ".gb", color: "#87AF87", maximum: 8 * 1024 * 1024},
	"gbc":   {extension: ".gbc", color: "#5F87D7", maximum: 8 * 1024 * 1024},
	"zx":    {extension: ".tap", color: "#AF87D7", maximum: 8 * 1024 * 1024},
	"chip8": {extension: ".ch8", color: "#5FD7D7", maximum: 65024},
}

type catalogEntry struct {
	ID     string
	Title  string
	System string
	ROM    string
	Color  string
}

type romStore struct {
	mu             sync.Mutex
	romRoot        string
	baseCatalog    string
	uploadCatalog  string
	restartCatalog func() error
}

func validTitle(title string) bool {
	if title != strings.TrimSpace(title) || title == "" || !utf8.ValidString(title) || utf8.RuneCountInString(title) > 64 {
		return false
	}
	for _, character := range title {
		if unicode.IsControl(character) || character == '\t' {
			return false
		}
	}
	return true
}

func slugify(title string) string {
	var builder strings.Builder
	lastHyphen := false
	for _, character := range strings.ToLower(title) {
		valid := character >= 'a' && character <= 'z' || character >= '0' && character <= '9'
		if valid {
			if builder.Len() >= 32 {
				break
			}
			builder.WriteRune(character)
			lastHyphen = false
		} else if builder.Len() > 0 && !lastHyphen && builder.Len() < 32 {
			builder.WriteByte('-')
			lastHyphen = true
		}
	}
	return strings.Trim(builder.String(), "-")
}

func validateNES(data []byte) error {
	if len(data) < 16 || !bytes.Equal(data[:4], []byte{'N', 'E', 'S', 0x1a}) {
		return errors.New("the file has no iNES header")
	}
	return nil
}

var nintendoLogo = []byte{
	0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b,
	0x03, 0x73, 0x00, 0x83, 0x00, 0x0c, 0x00, 0x0d,
	0x00, 0x08, 0x11, 0x1f, 0x88, 0x89, 0x00, 0x0e,
	0xdc, 0xcc, 0x6e, 0xe6, 0xdd, 0xdd, 0xd9, 0x99,
	0xbb, 0xbb, 0x67, 0x63, 0x6e, 0x0e, 0xec, 0xcc,
	0xdd, 0xdc, 0x99, 0x9f, 0xbb, 0xb9, 0x33, 0x3e,
}

func validateGameBoy(data []byte, colorOnly bool) error {
	if len(data) < 0x150 || !bytes.Equal(data[0x104:0x134], nintendoLogo) {
		return errors.New("the file has no valid Game Boy header")
	}
	checksum := byte(0)
	for index := 0x134; index <= 0x14c; index++ {
		checksum = checksum - data[index] - 1
	}
	if checksum != data[0x14d] {
		return errors.New("the Game Boy header checksum is invalid")
	}
	if colorOnly && data[0x143] != 0x80 && data[0x143] != 0xc0 {
		return errors.New("the ROM does not advertise Game Boy Color support")
	}
	return nil
}

func validateZX(data []byte) error {
	if len(data) < 4 {
		return errors.New("the TAP file is too short")
	}
	blocks := 0
	for offset := 0; offset < len(data); blocks++ {
		if len(data)-offset < 2 {
			return errors.New("the TAP file ends inside a block header")
		}
		blockSize := int(data[offset]) | int(data[offset+1])<<8
		offset += 2
		if blockSize < 2 || blockSize > len(data)-offset {
			return errors.New("the TAP file has an invalid block length")
		}
		checksum := byte(0)
		for _, value := range data[offset : offset+blockSize] {
			checksum ^= value
		}
		if checksum != 0 {
			return errors.New("the TAP file has an invalid block checksum")
		}
		offset += blockSize
	}
	if blocks == 0 {
		return errors.New("the TAP file contains no blocks")
	}
	return nil
}

func validateROM(system string, data []byte) error {
	spec, ok := supportedSystems[system]
	if !ok {
		return errors.New("choose a supported system")
	}
	if len(data) == 0 || len(data) > spec.maximum {
		return fmt.Errorf("the ROM must contain 1 through %d bytes", spec.maximum)
	}
	switch system {
	case "nes":
		return validateNES(data)
	case "gb":
		return validateGameBoy(data, false)
	case "gbc":
		return validateGameBoy(data, true)
	case "zx":
		return validateZX(data)
	default:
		return nil
	}
}

func decodeUpload(system, filename string, input io.Reader) ([]byte, error) {
	spec, ok := supportedSystems[system]
	if !ok {
		return nil, errors.New("choose a supported system")
	}
	archive, err := io.ReadAll(io.LimitReader(input, maximumArchiveBytes+1))
	if err != nil {
		return nil, errors.New("the upload could not be read")
	}
	if len(archive) > maximumArchiveBytes {
		return nil, errors.New("the upload exceeds 10 MiB")
	}
	extension := strings.ToLower(filepath.Ext(filename))
	if extension == spec.extension {
		if err := validateROM(system, archive); err != nil {
			return nil, err
		}
		return archive, nil
	}
	if extension != ".zip" {
		return nil, fmt.Errorf("choose a %s file or a ZIP containing one", spec.extension)
	}
	reader, err := zip.NewReader(bytes.NewReader(archive), int64(len(archive)))
	if err != nil {
		return nil, errors.New("the ZIP file is malformed")
	}
	var member *zip.File
	for _, candidate := range reader.File {
		if candidate.FileInfo().IsDir() {
			continue
		}
		if member != nil {
			return nil, errors.New("the ZIP must contain exactly one ROM")
		}
		member = candidate
	}
	if member == nil || member.Flags&1 != 0 || member.Mode()&os.ModeSymlink != 0 ||
		member.Name != filepath.Base(member.Name) || strings.Contains(member.Name, "\\") {
		return nil, errors.New("the ZIP contains an unsafe or encrypted member")
	}
	if strings.ToLower(filepath.Ext(member.Name)) != spec.extension || member.UncompressedSize64 > uint64(spec.maximum) {
		return nil, fmt.Errorf("the ZIP must contain one %s file of a valid size", spec.extension)
	}
	memberReader, err := member.Open()
	if err != nil {
		return nil, errors.New("the ZIP member could not be opened")
	}
	defer memberReader.Close()
	data, err := io.ReadAll(io.LimitReader(memberReader, int64(spec.maximum)+1))
	if err != nil || len(data) > spec.maximum {
		return nil, errors.New("the ZIP member could not be read safely")
	}
	if err := validateROM(system, data); err != nil {
		return nil, err
	}
	return data, nil
}

func parseCatalog(path string, missingOK bool) ([]catalogEntry, error) {
	pathInfo, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) && missingOK {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if !pathInfo.Mode().IsRegular() || pathInfo.Mode()&os.ModeSymlink != 0 || pathInfo.Size() < 0 || pathInfo.Size() > maximumCatalogBytes {
		return nil, errors.New("catalog is not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(pathInfo, info) {
		return nil, errors.New("catalog is not a bounded regular file")
	}
	entries := make([]catalogEntry, 0)
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 4096), 4097)
	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 5 {
			return nil, errors.New("catalog contains a malformed line")
		}
		entries = append(entries, catalogEntry{ID: fields[0], Title: fields[1], System: fields[2], ROM: fields[3], Color: fields[4]})
		if len(entries) > maximumGames {
			return nil, errors.New("catalog contains too many games")
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	return entries, nil
}

func encodeCatalog(entries []catalogEntry) []byte {
	var builder strings.Builder
	for _, entry := range entries {
		fmt.Fprintf(&builder, "%s\t%s\t%s\t%s\t%s\n", entry.ID, entry.Title, entry.System, entry.ROM, entry.Color)
	}
	return []byte(builder.String())
}

func ensureDirectory(path string, mode os.FileMode) error {
	if err := os.MkdirAll(path, mode); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("storage path is not a real directory")
	}
	return nil
}

func installExclusive(path string, data []byte) error {
	directory := filepath.Dir(path)
	temporary, err := os.CreateTemp(directory, ".rom-upload-*")
	if err != nil {
		return err
	}
	temporaryName := temporary.Name()
	defer os.Remove(temporaryName)
	if err := temporary.Chmod(0600); err != nil {
		temporary.Close()
		return err
	}
	if _, err := temporary.Write(data); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Sync(); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Link(temporaryName, path); err != nil {
		if errors.Is(err, os.ErrExist) {
			return errors.New("a ROM with this name already exists")
		}
		return err
	}
	installed := true
	defer func() {
		if !installed {
			_ = os.Remove(path)
		}
	}()
	directoryHandle, err := os.Open(directory)
	if err != nil {
		installed = false
		return err
	}
	defer directoryHandle.Close()
	if err := directoryHandle.Sync(); err != nil {
		installed = false
		return err
	}
	return nil
}

func (store *romStore) entries() ([]catalogEntry, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	return parseCatalog(store.uploadCatalog, true)
}

func (store *romStore) add(system, title, filename string, input io.Reader) (catalogEntry, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	if !validTitle(title) {
		return catalogEntry{}, errors.New("enter a title of 1 through 64 printable characters")
	}
	spec, ok := supportedSystems[system]
	if !ok {
		return catalogEntry{}, errors.New("choose a supported system")
	}
	slug := slugify(title)
	if slug == "" {
		return catalogEntry{}, errors.New("the title needs at least one ASCII letter or number for its filename")
	}
	data, err := decodeUpload(system, filename, input)
	if err != nil {
		return catalogEntry{}, err
	}
	baseEntries, err := parseCatalog(store.baseCatalog, false)
	if err != nil {
		return catalogEntry{}, fmt.Errorf("read built-in catalog: %w", err)
	}
	uploadEntries, err := parseCatalog(store.uploadCatalog, true)
	if err != nil {
		return catalogEntry{}, fmt.Errorf("read upload catalog: %w", err)
	}
	if len(baseEntries)+len(uploadEntries) >= maximumGames {
		return catalogEntry{}, errors.New("the menu catalog is full")
	}
	identifier := "upload-" + system + "-" + slug
	if len(identifier) > 48 {
		identifier = identifier[:48]
		identifier = strings.TrimRight(identifier, "-")
	}
	directory := filepath.Join(store.romRoot, system)
	if err := ensureDirectory(directory, 0755); err != nil {
		return catalogEntry{}, fmt.Errorf("prepare ROM directory: %w", err)
	}
	destination := filepath.Join(directory, slug+spec.extension)
	for _, entry := range append(baseEntries, uploadEntries...) {
		if entry.ID == identifier || entry.ROM == destination {
			return catalogEntry{}, errors.New("a game with this title is already cataloged")
		}
	}
	if err := installExclusive(destination, data); err != nil {
		return catalogEntry{}, err
	}
	entry := catalogEntry{ID: identifier, Title: title, System: system, ROM: destination, Color: spec.color}
	uploadEntries = append(uploadEntries, entry)
	sort.SliceStable(uploadEntries, func(left, right int) bool {
		if uploadEntries[left].System == uploadEntries[right].System {
			return uploadEntries[left].Title < uploadEntries[right].Title
		}
		return uploadEntries[left].System < uploadEntries[right].System
	})
	if err := atomicWrite(store.uploadCatalog, encodeCatalog(uploadEntries), 0600); err != nil {
		_ = os.Remove(destination)
		return catalogEntry{}, fmt.Errorf("save upload catalog: %w", err)
	}
	if store.restartCatalog != nil {
		if err := store.restartCatalog(); err != nil {
			return entry, fmt.Errorf("ROM saved, but the dashboard could not reload: %w", err)
		}
	}
	return entry, nil
}
