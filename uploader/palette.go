package main

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
)

const maximumPaletteBytes = 4096

type paletteSpec struct {
	name  string
	label string
}

type settingsIconSpec struct {
	name     string
	label    string
	family   string
	imageURL string
	rows     []string
}

const defaultSettingsIcon = "gear-knekko-09"

var baseSettingsIconSpecs = []settingsIconSpec{
	{name: "gear-classic", label: "Classic", rows: []string{"..##.##..", ".#######.", "###...###", "##.....##", "##.....##", "##.....##", "###...###", ".#######.", "..##.##.."}},
	{name: "gear-square", label: "Square", rows: []string{".##...##.", ".##...##.", "#########", "##.....##", "##.....##", "##.....##", "#########", ".##...##.", ".##...##."}},
	{name: "gear-diamond", label: "Diamond", rows: []string{"....#....", "..#####..", ".##...##.", "##.....##", "#.......#", "##.....##", ".##...##.", "..#####..", "....#...."}},
	{name: "gear-eight", label: "Eight tooth", rows: []string{".##...##.", "###...###", ".#######.", "..#...#..", "..#...#..", "..#...#..", ".#######.", "###...###", ".##...##."}},
	{name: "gear-spoke", label: "Spoked", rows: []string{"...###...", ".#.###.#.", "..#####..", "###.#.###", "####.####", "###.#.###", "..#####..", ".#.###.#.", "...###..."}},
	{name: "gear-ring", label: "Ring", rows: []string{"...###...", ".#######.", "###...###", "##.....##", "##.....##", "##.....##", "###...###", ".#######.", "...###..."}},
	{name: "gear-cross", label: "Cross", rows: []string{"...###...", "...###...", "..#####..", "###...###", "###...###", "###...###", "..#####..", "...###...", "...###..."}},
	{name: "gear-compact", label: "Compact", rows: []string{".........", "...###...", "..#####..", ".##...##.", ".##...##.", ".##...##.", "..#####..", "...###...", "........."}},
	{name: "gear-heavy", label: "Heavy", rows: []string{".###.###.", "#########", "###...###", "##.....##", "##.....##", "##.....##", "###...###", "#########", ".###.###."}},
	{name: "gear-rivet", label: "Riveted", rows: []string{"..#...#..", ".#######.", "##.#.#.##", ".#.....#.", ".#.....#.", ".#.....#.", "##.#.#.##", ".#######.", "..#...#.."}},
	{name: "gear-outline", label: "Classic outline", rows: []string{"..##.##..", "..#...#..", "##.###.##", "#.#...#.#", "#.#...#.#", "#.#...#.#", "##.###.##", "..#...#..", "..##.##.."}},
	{name: "gear-steel-outline", label: "Outline", family: "current", rows: []string{
		".......................", ".......#.......#.......", ".......##.....##.......", ".......####.####.......",
		".......#########.......", "......###########......", "......###.....###......", "..######.......######..",
		"..#####.........#####..", "...###...........###...", "....##...........##....", ".....#...........#.....",
		"....##...........##....", "...###...........###...", "..#####.........#####..", "..######.......######..",
		"......###.....###......", "......###########......", ".......#########.......", ".......####.####.......",
		".......##.....##.......", ".......#.......#.......", ".......................",
	}},
}

var settingsIconSpecs = append(baseSettingsIconSpecs, knekkoSettingsIconSpecs...)

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
	Value string
}

type settingsIconField struct {
	Name     string
	Label    string
	Pixels   []bool
	GridSize int
	Family   string
	ImageURL string
	Selected bool
}

type settingsIconGroup struct {
	Label string
	Icons []settingsIconField
}

type dashboardAppearance struct {
	palette      map[string]string
	settingsIcon string
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

func validSettingsIcon(name string) bool {
	for _, spec := range settingsIconSpecs {
		if spec.name == name {
			return true
		}
	}
	return false
}

func normalizeRGB(value string) (string, bool) {
	if len(value) != 7 || value[0] != '#' {
		return "", false
	}
	for index := 1; index < len(value); index++ {
		character := value[index]
		if !((character >= '0' && character <= '9') ||
			(character >= 'a' && character <= 'f') ||
			(character >= 'A' && character <= 'F')) {
			return "", false
		}
	}
	return strings.ToUpper(value), true
}

func validatePalette(values map[string]string) error {
	if len(values) != len(dashboardPaletteSpecs) {
		return errors.New("palette must contain every dashboard color exactly once")
	}
	for _, spec := range dashboardPaletteSpecs {
		value, ok := values[spec.name]
		if !ok {
			return fmt.Errorf("palette is missing %s", spec.name)
		}
		if normalized, valid := normalizeRGB(value); !valid || normalized != value {
			return fmt.Errorf("palette %s must use canonical #RRGGBB", spec.name)
		}
	}
	return nil
}

func parsePaletteTSV(contents []byte) (dashboardAppearance, error) {
	values := make(map[string]string, len(dashboardPaletteSpecs))
	settingsIcon := ""
	scanner := bufio.NewScanner(strings.NewReader(string(contents)))
	scanner.Buffer(make([]byte, maximumPaletteBytes), maximumPaletteBytes+1)
	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		fields := strings.Split(line, "\t")
		if len(fields) != 2 {
			return dashboardAppearance{}, errors.New("appearance contains a malformed entry")
		}
		if fields[0] == "settings-icon" {
			if settingsIcon != "" || !validSettingsIcon(fields[1]) {
				return dashboardAppearance{}, errors.New("appearance contains an invalid settings icon")
			}
			settingsIcon = fields[1]
			continue
		}
		if !validPaletteName(fields[0]) {
			return dashboardAppearance{}, errors.New("palette contains an unknown entry")
		}
		if _, exists := values[fields[0]]; exists {
			return dashboardAppearance{}, fmt.Errorf("palette repeats %s", fields[0])
		}
		value, valid := normalizeRGB(fields[1])
		if !valid {
			return dashboardAppearance{}, fmt.Errorf("palette %s is not a full RGB color", fields[0])
		}
		values[fields[0]] = value
	}
	if err := scanner.Err(); err != nil {
		return dashboardAppearance{}, err
	}
	if err := validatePalette(values); err != nil {
		return dashboardAppearance{}, err
	}
	if settingsIcon == "" {
		settingsIcon = defaultSettingsIcon
	}
	return dashboardAppearance{palette: values, settingsIcon: settingsIcon}, nil
}

func paletteTokens(contents []byte) ([]string, error) {
	if len(contents) == 0 || len(contents) > maximumPaletteBytes {
		return nil, errors.New("palette override has an invalid size")
	}
	tokens := make([]string, 0, 2*len(dashboardPaletteSpecs)+10)
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
		if len(tokens) > 2*len(dashboardPaletteSpecs)+10 {
			return nil, errors.New("palette override contains too many tokens")
		}
	}
	return tokens, nil
}

func parsePaletteOverride(contents []byte) (dashboardAppearance, error) {
	tokens, err := paletteTokens(contents)
	if err != nil {
		return dashboardAppearance{}, err
	}
	if len(tokens) < 7 || tokens[0] != "(" || tokens[1] != ":version" || tokens[len(tokens)-2] != ")" || tokens[len(tokens)-1] != ")" {
		return dashboardAppearance{}, errors.New("appearance override has an invalid structure")
	}
	version := tokens[2]
	index := 3
	settingsIcon := ""
	if version == "3" {
		if len(tokens) != 2*len(dashboardPaletteSpecs)+9 || tokens[index] != ":settings-icon" {
			return dashboardAppearance{}, errors.New("appearance override must use schema version 3")
		}
		encodedIcon := tokens[index+1]
		if len(encodedIcon) < 3 || encodedIcon[0] != '"' || encodedIcon[len(encodedIcon)-1] != '"' || !validSettingsIcon(encodedIcon[1:len(encodedIcon)-1]) {
			return dashboardAppearance{}, errors.New("appearance override contains an unknown settings icon")
		}
		settingsIcon = encodedIcon[1 : len(encodedIcon)-1]
		index += 2
	} else if version == "2" {
		if len(tokens) != 2*len(dashboardPaletteSpecs)+7 {
			return dashboardAppearance{}, errors.New("palette override version 2 has an invalid structure")
		}
	} else {
		return dashboardAppearance{}, errors.New("appearance override must use schema version 2 or 3")
	}
	if tokens[index] != ":palette" || tokens[index+1] != "(" {
		return dashboardAppearance{}, errors.New("appearance override is missing its palette")
	}
	index += 2
	values := make(map[string]string, len(dashboardPaletteSpecs))
	for ; index < len(tokens)-2; index += 2 {
		key := tokens[index]
		if !strings.HasPrefix(key, ":") || !validPaletteName(key[1:]) {
			return dashboardAppearance{}, errors.New("palette override contains an unknown color")
		}
		name := key[1:]
		if _, exists := values[name]; exists {
			return dashboardAppearance{}, fmt.Errorf("palette override repeats %s", name)
		}
		encoded := tokens[index+1]
		if len(encoded) != 9 || encoded[0] != '"' || encoded[8] != '"' {
			return dashboardAppearance{}, fmt.Errorf("palette override %s is not a quoted RGB color", name)
		}
		value, valid := normalizeRGB(encoded[1:8])
		if !valid {
			return dashboardAppearance{}, fmt.Errorf("palette override %s is not a full RGB color", name)
		}
		values[name] = value
	}
	if err := validatePalette(values); err != nil {
		return dashboardAppearance{}, err
	}
	return dashboardAppearance{palette: values, settingsIcon: settingsIcon}, nil
}

func encodePaletteOverride(values map[string]string, settingsIcon string) []byte {
	var builder strings.Builder
	fmt.Fprintf(&builder, "(:version 3\n :settings-icon %q\n :palette\n  (", settingsIcon)
	for index, spec := range dashboardPaletteSpecs {
		if index > 0 {
			builder.WriteString("\n   ")
		}
		fmt.Fprintf(&builder, ":%s \"%s\"", spec.name, values[spec.name])
	}
	builder.WriteString("))\n")
	return []byte(builder.String())
}

func paletteFields(values map[string]string) []paletteField {
	fields := make([]paletteField, 0, len(dashboardPaletteSpecs))
	for _, spec := range dashboardPaletteSpecs {
		fields = append(fields, paletteField{Name: spec.name, Label: spec.label, Value: values[spec.name]})
	}
	return fields
}

func settingsIconFields(selected string) []settingsIconField {
	fields := make([]settingsIconField, 0, len(settingsIconSpecs))
	for _, spec := range settingsIconSpecs {
		pixels := make([]bool, 0, len(spec.rows)*len(spec.rows))
		for _, row := range spec.rows {
			for _, pixel := range row {
				pixels = append(pixels, pixel == '#')
			}
		}
		fields = append(fields, settingsIconField{Name: spec.name, Label: spec.label, Pixels: pixels, GridSize: len(spec.rows), Family: spec.family, ImageURL: spec.imageURL, Selected: spec.name == selected})
	}
	return fields
}

func settingsIconGroups(fields []settingsIconField) []settingsIconGroup {
	order := []struct {
		family string
		label  string
	}{
		{family: "legacy", label: "Legacy selection"},
		{family: "current", label: "Current"},
		{family: "small", label: "Knekko small"},
		{family: "medium", label: "Knekko medium"},
		{family: "large", label: "Knekko large"},
	}
	groups := make([]settingsIconGroup, 0, len(order))
	for _, definition := range order {
		group := settingsIconGroup{Label: definition.label}
		for _, field := range fields {
			family := field.Family
			if family == "" {
				family = "legacy"
			}
			if family != definition.family || (family == "legacy" && !field.Selected) {
				continue
			}
			group.Icons = append(group.Icons, field)
		}
		if len(group.Icons) > 0 {
			groups = append(groups, group)
		}
	}
	return groups
}

func (store *paletteStore) currentLocked() ([]paletteField, []settingsIconField, error) {
	var appearance dashboardAppearance
	var loaded bool
	var baseError error
	// The launcher deliberately chooses the checked-in appearance when an override
	// fails. Prefer that same source here so an older generated file cannot make
	// the web form claim stale colors are active.
	for _, path := range []string{store.fallbackPath, store.activePath} {
		contents, err := readBoundedRegular(path, maximumPaletteBytes)
		if err != nil {
			baseError = err
			continue
		}
		appearance, err = parsePaletteTSV(contents)
		if err == nil {
			loaded = true
			break
		}
		baseError = err
	}
	if !loaded {
		return nil, nil, fmt.Errorf("read installed dashboard appearance: %w", baseError)
	}
	contents, err := readBoundedRegular(store.overridePath, maximumPaletteBytes)
	if err == nil {
		if override, parseErr := parsePaletteOverride(contents); parseErr == nil {
			appearance.palette = override.palette
			if override.settingsIcon != "" {
				appearance.settingsIcon = override.settingsIcon
			}
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		// A bad optional override must never hide the usable installed palette.
	}
	return paletteFields(appearance.palette), settingsIconFields(appearance.settingsIcon), nil
}

func (store *paletteStore) current() ([]paletteField, []settingsIconField, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	return store.currentLocked()
}

func (store *paletteStore) save(values map[string]string, settingsIcon string) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if err := validatePalette(values); err != nil {
		return err
	}
	if !validSettingsIcon(settingsIcon) {
		return errors.New("choose one of the available settings icons")
	}
	if err := atomicWrite(store.overridePath, encodePaletteOverride(values, settingsIcon), 0600); err != nil {
		return fmt.Errorf("save dashboard appearance: %w", err)
	}
	if store.restartDashboard != nil {
		if err := store.restartDashboard(); err != nil {
			return fmt.Errorf("appearance was saved, but the dashboard could not reload: %w", err)
		}
	}
	return nil
}
