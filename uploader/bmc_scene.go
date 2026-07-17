package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
)

const retroDeckWidgetUID = "73219c9d-f1ef-41dc-960c-d0711e42a6ac"

const maximumBMCConfigSize = 4 * 1024 * 1024

type bmcSceneWidget struct {
	ID            string         `json:"id"`
	Row           int            `json:"row"`
	Col           int            `json:"col"`
	Placement     string         `json:"placement"`
	WidgetTypeID  string         `json:"widget_type_id"`
	ViewportShape string         `json:"viewport_shape"`
	Params        map[string]any `json:"params"`
}

type bmcScene struct {
	ID      string           `json:"id"`
	Enabled bool             `json:"enabled"`
	Kind    string           `json:"kind"`
	Widgets []bmcSceneWidget `json:"widgets"`
}

func randomUUID() (string, error) {
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", fmt.Errorf("generate UUID: %w", err)
	}
	value[6] = (value[6] & 0x0f) | 0x40
	value[8] = (value[8] & 0x3f) | 0x80
	encoded := hex.EncodeToString(value[:])
	return encoded[0:8] + "-" + encoded[8:12] + "-" + encoded[12:16] + "-" +
		encoded[16:20] + "-" + encoded[20:32], nil
}

func installBMCScene(path string) (bool, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return false, fmt.Errorf("stat BMC configuration: %w", err)
	}
	if !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > maximumBMCConfigSize {
		return false, errors.New("BMC configuration is not a bounded regular file")
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		return false, fmt.Errorf("read BMC configuration: %w", err)
	}
	var config map[string]json.RawMessage
	if err := json.Unmarshal(contents, &config); err != nil {
		return false, fmt.Errorf("parse BMC configuration: %w", err)
	}
	var scenes []json.RawMessage
	rawScenes, present := config["scenes"]
	if !present {
		return false, errors.New("BMC configuration has no scene list")
	}
	if err := json.Unmarshal(rawScenes, &scenes); err != nil {
		return false, fmt.Errorf("parse BMC scene list: %w", err)
	}
	for _, rawScene := range scenes {
		var scene struct {
			Widgets []struct {
				WidgetTypeID string `json:"widget_type_id"`
			} `json:"widgets"`
		}
		if err := json.Unmarshal(rawScene, &scene); err != nil {
			return false, fmt.Errorf("parse BMC scene: %w", err)
		}
		for _, widget := range scene.Widgets {
			if widget.WidgetTypeID == retroDeckWidgetUID {
				return false, nil
			}
		}
	}

	sceneID, err := randomUUID()
	if err != nil {
		return false, err
	}
	widgetID, err := randomUUID()
	if err != nil {
		return false, err
	}
	newScene, err := json.Marshal(bmcScene{
		ID:      sceneID,
		Enabled: true,
		Kind:    "fullscreen",
		Widgets: []bmcSceneWidget{{
			ID:            widgetID,
			Row:           0,
			Col:           0,
			Placement:     "fullscreen",
			WidgetTypeID:  retroDeckWidgetUID,
			ViewportShape: "rectangular",
			Params:        map[string]any{},
		}},
	})
	if err != nil {
		return false, fmt.Errorf("encode Retro Deck scene: %w", err)
	}
	scenes = append(scenes, newScene)
	config["scenes"], err = json.Marshal(scenes)
	if err != nil {
		return false, fmt.Errorf("encode BMC scene list: %w", err)
	}
	updated, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		return false, fmt.Errorf("encode BMC configuration: %w", err)
	}
	updated = append(updated, '\n')

	backup := path + ".retro-deck.bak"
	backupInfo, err := os.Lstat(backup)
	if errors.Is(err, os.ErrNotExist) {
		if err := atomicWrite(backup, contents, info.Mode().Perm()); err != nil {
			return false, fmt.Errorf("back up BMC configuration: %w", err)
		}
	} else if err != nil {
		return false, fmt.Errorf("stat BMC configuration backup: %w", err)
	} else if !backupInfo.Mode().IsRegular() {
		return false, errors.New("BMC configuration backup is not a regular file")
	}
	if err := atomicWrite(path, updated, info.Mode().Perm()); err != nil {
		return false, fmt.Errorf("write BMC configuration: %w", err)
	}
	return true, nil
}
