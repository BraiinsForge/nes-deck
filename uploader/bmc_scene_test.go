package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestInstallBMCScene(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bmc_config.json")
	original := []byte(`{"scenes":[],"sound":{"volume":42}}`)
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	changed, err := installBMCScene(path)
	if err != nil {
		t.Fatal(err)
	}
	if !changed {
		t.Fatal("first installation did not change the configuration")
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var config struct {
		Scenes []bmcScene `json:"scenes"`
		Sound  struct {
			Volume int `json:"volume"`
		} `json:"sound"`
	}
	if err := json.Unmarshal(contents, &config); err != nil {
		t.Fatal(err)
	}
	if len(config.Scenes) != 1 || len(config.Scenes[0].Widgets) != 1 {
		t.Fatalf("unexpected scene shape: %+v", config.Scenes)
	}
	widget := config.Scenes[0].Widgets[0]
	if widget.WidgetTypeID != retroDeckWidgetUID || widget.Placement != "fullscreen" {
		t.Fatalf("unexpected widget: %+v", widget)
	}
	if config.Sound.Volume != 42 {
		t.Fatalf("unrelated configuration changed: %+v", config.Sound)
	}
	backup, err := os.ReadFile(path + ".retro-deck.bak")
	if err != nil {
		t.Fatal(err)
	}
	if string(backup) != string(original) {
		t.Fatalf("backup changed: %q", backup)
	}

	changed, err = installBMCScene(path)
	if err != nil {
		t.Fatal(err)
	}
	if changed {
		t.Fatal("repeated installation changed the configuration")
	}
}
