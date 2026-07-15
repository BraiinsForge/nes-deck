package main

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"strconv"
	"strings"
	"sync"
)

const maximumPaletteBytes = 4096

type paletteSpec struct {
	name  string
	label string
}

var dashboardPaletteSpecs = []paletteSpec{
	{name: "background", label: "Background"},
	{name: "text-dark", label: "Dark text"},
	{name: "field", label: "Field"},
	{name: "surface", label: "Surface"},
	{name: "inactive-border", label: "Inactive border"},
	{name: "control-border", label: "Control border"},
	{name: "footer", label: "Footer"},
	{name: "inactive-text", label: "Inactive text"},
	{name: "text", label: "Text"},
	{name: "white", label: "Bright white"},
	{name: "title", label: "Title"},
	{name: "volume-off", label: "Volume off"},
	{name: "volume-on", label: "Volume on"},
	{name: "selected", label: "Selected item"},
	{name: "wifi-active", label: "Wi-Fi active"},
	{name: "wifi-focus", label: "Wi-Fi focus"},
	{name: "wifi-active-border", label: "Wi-Fi active border"},
	{name: "field-label", label: "Field label"},
	{name: "accent", label: "Accent"},
	{name: "active", label: "Active control"},
	{name: "control-surface", label: "Control surface"},
	{name: "muted", label: "Muted control"},
}

type paletteField struct {
	Name  string
	Label string
	Value int
}

type paletteStore struct {
	mu               sync.Mutex
	activePath       string
	fallbackPath     string
	overridePath     string
	restartDashboard func() error
}

func readBoundedRegular(path string, maximum int64) ([]byte, error) {
	pathInfo, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !pathInfo.Mode().IsRegular() || pathInfo.Mode()&os.ModeSymlink != 0 || pathInfo.Size() < 0 || pathInfo.Size() > maximum {
		return nil, errors.New("file is not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || !os.SameFile(pathInfo, info) {
		return nil, errors.New("file changed while it was opened")
	}
	contents, err := io.ReadAll(file)
	if err != nil {
		return nil, err
	}
	if int64(len(contents)) > maximum {
		return nil, errors.New("file exceeds its size limit")
	}
	return contents, nil
}

func validPaletteName(name string) bool {
	for _, spec := range dashboardPaletteSpecs {
		if spec.name == name {
			return true
		}
	}
	return false
}

func validatePalette(values map[string]int) error {
	if len(values) != len(dashboardPaletteSpecs) {
		return errors.New("palette must contain every dashboard color exactly once")
	}
	for _, spec := range dashboardPaletteSpecs {
		value, ok := values[spec.name]
		if !ok {
			return fmt.Errorf("palette is missing %s", spec.name)
		}
		if value < 0 || value > 255 {
			return fmt.Errorf("palette %s must be from 0 through 255", spec.name)
		}
	}
	return nil
}

func parsePaletteTSV(contents []byte) (map[string]int, error) {
	values := make(map[string]int, len(dashboardPaletteSpecs))
	scanner := bufio.NewScanner(strings.NewReader(string(contents)))
	scanner.Buffer(make([]byte, maximumPaletteBytes), maximumPaletteBytes+1)
	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		fields := strings.Split(line, "\t")
		if len(fields) != 2 || !validPaletteName(fields[0]) {
			return nil, errors.New("palette contains a malformed or unknown entry")
		}
		if _, exists := values[fields[0]]; exists {
			return nil, fmt.Errorf("palette repeats %s", fields[0])
		}
		value, err := strconv.Atoi(fields[1])
		if err != nil {
			return nil, fmt.Errorf("palette %s is not an integer", fields[0])
		}
		values[fields[0]] = value
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	if err := validatePalette(values); err != nil {
		return nil, err
	}
	return values, nil
}

func paletteTokens(contents []byte) ([]string, error) {
	if len(contents) == 0 || len(contents) > maximumPaletteBytes {
		return nil, errors.New("palette override has an invalid size")
	}
	tokens := make([]string, 0, 2*len(dashboardPaletteSpecs)+6)
	for index := 0; index < len(contents); {
		character := contents[index]
		if character == ' ' || character == '\t' || character == '\r' || character == '\n' {
			index++
			continue
		}
		if character == '(' || character == ')' {
			tokens = append(tokens, string(character))
			index++
			continue
		}
		start := index
		for index < len(contents) {
			character = contents[index]
			if character == ' ' || character == '\t' || character == '\r' || character == '\n' || character == '(' || character == ')' {
				break
			}
			index++
		}
		if start == index {
			return nil, errors.New("palette override contains an invalid token")
		}
		token := string(contents[start:index])
		if len(token) > 64 {
			return nil, errors.New("palette override token is too long")
		}
		tokens = append(tokens, token)
		if len(tokens) > 2*len(dashboardPaletteSpecs)+8 {
			return nil, errors.New("palette override contains too many tokens")
		}
	}
	return tokens, nil
}

func parsePaletteOverride(contents []byte) (map[string]int, error) {
	tokens, err := paletteTokens(contents)
	if err != nil {
		return nil, err
	}
	if len(tokens) != 2*len(dashboardPaletteSpecs)+7 || tokens[0] != "(" || tokens[1] != ":version" || tokens[2] != "1" || tokens[3] != ":palette" || tokens[4] != "(" || tokens[len(tokens)-2] != ")" || tokens[len(tokens)-1] != ")" {
		return nil, errors.New("palette override must use schema version 1")
	}
	values := make(map[string]int, len(dashboardPaletteSpecs))
	for index := 5; index < len(tokens)-2; index += 2 {
		key := tokens[index]
		if !strings.HasPrefix(key, ":") || !validPaletteName(key[1:]) {
			return nil, errors.New("palette override contains an unknown color")
		}
		name := key[1:]
		if _, exists := values[name]; exists {
			return nil, fmt.Errorf("palette override repeats %s", name)
		}
		value, err := strconv.Atoi(tokens[index+1])
		if err != nil {
			return nil, fmt.Errorf("palette override %s is not an integer", name)
		}
		values[name] = value
	}
	if err := validatePalette(values); err != nil {
		return nil, err
	}
	return values, nil
}

func encodePaletteOverride(values map[string]int) []byte {
	var builder strings.Builder
	builder.WriteString("(:version 1\n :palette\n  (")
	for index, spec := range dashboardPaletteSpecs {
		if index > 0 {
			builder.WriteString("\n   ")
		}
		fmt.Fprintf(&builder, ":%s %d", spec.name, values[spec.name])
	}
	builder.WriteString("))\n")
	return []byte(builder.String())
}

func paletteFields(values map[string]int) []paletteField {
	fields := make([]paletteField, 0, len(dashboardPaletteSpecs))
	for _, spec := range dashboardPaletteSpecs {
		fields = append(fields, paletteField{Name: spec.name, Label: spec.label, Value: values[spec.name]})
	}
	return fields
}

func (store *paletteStore) currentLocked() ([]paletteField, error) {
	var values map[string]int
	var baseError error
	// The launcher deliberately chooses the checked-in palette when an override
	// fails. Prefer that same source here so an older generated file cannot make
	// the web form claim stale colors are active.
	for _, path := range []string{store.fallbackPath, store.activePath} {
		contents, err := readBoundedRegular(path, maximumPaletteBytes)
		if err != nil {
			baseError = err
			continue
		}
		values, err = parsePaletteTSV(contents)
		if err == nil {
			break
		}
		baseError = err
	}
	if values == nil {
		return nil, fmt.Errorf("read installed dashboard palette: %w", baseError)
	}
	contents, err := readBoundedRegular(store.overridePath, maximumPaletteBytes)
	if err == nil {
		if override, parseErr := parsePaletteOverride(contents); parseErr == nil {
			values = override
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		// A bad optional override must never hide the usable installed palette.
	}
	return paletteFields(values), nil
}

func (store *paletteStore) current() ([]paletteField, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	return store.currentLocked()
}

func (store *paletteStore) save(values map[string]int) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if err := validatePalette(values); err != nil {
		return err
	}
	if err := atomicWrite(store.overridePath, encodePaletteOverride(values), 0600); err != nil {
		return fmt.Errorf("save dashboard palette: %w", err)
	}
	if store.restartDashboard != nil {
		if err := store.restartDashboard(); err != nil {
			return fmt.Errorf("colors were saved, but the dashboard could not reload: %w", err)
		}
	}
	return nil
}
