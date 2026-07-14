package main

import (
	"bufio"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
)

const (
	passwordIterations  = 210000
	minimumPasswordSize = 8
	maximumPasswordSize = 128
)

type passwordConfig struct {
	iterations int
	salt       []byte
	digest     []byte
}

func derivePassword(password, salt []byte, iterations int) []byte {
	block := make([]byte, len(salt)+4)
	copy(block, salt)
	block[len(salt)+3] = 1
	mac := hmac.New(sha256.New, password)
	_, _ = mac.Write(block)
	u := mac.Sum(nil)
	result := append([]byte(nil), u...)
	for round := 1; round < iterations; round++ {
		mac.Reset()
		_, _ = mac.Write(u)
		u = mac.Sum(u[:0])
		for index := range result {
			result[index] ^= u[index]
		}
	}
	return result
}

func newPasswordConfig(password string) (passwordConfig, error) {
	if len(password) < minimumPasswordSize || len(password) > maximumPasswordSize {
		return passwordConfig{}, fmt.Errorf("password must contain %d through %d bytes", minimumPasswordSize, maximumPasswordSize)
	}
	salt := make([]byte, 16)
	if _, err := io.ReadFull(rand.Reader, salt); err != nil {
		return passwordConfig{}, fmt.Errorf("generate password salt: %w", err)
	}
	return passwordConfig{
		iterations: passwordIterations,
		salt:       salt,
		digest:     derivePassword([]byte(password), salt, passwordIterations),
	}, nil
}

func (config passwordConfig) matches(password string) bool {
	if len(password) > maximumPasswordSize || len(config.digest) != sha256.Size {
		return false
	}
	candidate := derivePassword([]byte(password), config.salt, config.iterations)
	return subtle.ConstantTimeCompare(candidate, config.digest) == 1
}

func encodePasswordConfig(config passwordConfig) []byte {
	return []byte(fmt.Sprintf("version=1\niterations=%d\nsalt=%s\ndigest=%s\n",
		config.iterations,
		base64.RawStdEncoding.EncodeToString(config.salt),
		base64.RawStdEncoding.EncodeToString(config.digest)))
}

func parsePasswordConfig(contents []byte) (passwordConfig, error) {
	if len(contents) == 0 || len(contents) > 1024 {
		return passwordConfig{}, errors.New("password configuration has an invalid size")
	}
	lines := strings.Split(string(contents), "\n")
	if len(lines) != 5 || lines[0] != "version=1" || lines[4] != "" {
		return passwordConfig{}, errors.New("password configuration has an invalid schema")
	}
	values := make(map[string]string, 3)
	for _, line := range lines[1:4] {
		parts := strings.SplitN(line, "=", 2)
		if len(parts) != 2 || values[parts[0]] != "" {
			return passwordConfig{}, errors.New("password configuration has duplicate or malformed fields")
		}
		values[parts[0]] = parts[1]
	}
	iterations, err := strconv.Atoi(values["iterations"])
	if err != nil || iterations < 100000 || iterations > 1000000 {
		return passwordConfig{}, errors.New("password configuration has invalid iterations")
	}
	salt, err := base64.RawStdEncoding.DecodeString(values["salt"])
	if err != nil || len(salt) != 16 {
		return passwordConfig{}, errors.New("password configuration has an invalid salt")
	}
	digest, err := base64.RawStdEncoding.DecodeString(values["digest"])
	if err != nil || len(digest) != sha256.Size {
		return passwordConfig{}, errors.New("password configuration has an invalid digest")
	}
	return passwordConfig{iterations: iterations, salt: salt, digest: digest}, nil
}

func loadPasswordConfig(path string) (passwordConfig, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return passwordConfig{}, fmt.Errorf("inspect password configuration: %w", err)
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 {
		return passwordConfig{}, errors.New("password configuration must be a private regular file")
	}
	if stat, ok := info.Sys().(*syscall.Stat_t); ok && os.Geteuid() == 0 && stat.Uid != 0 {
		return passwordConfig{}, errors.New("password configuration must be owned by root")
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		return passwordConfig{}, fmt.Errorf("read password configuration: %w", err)
	}
	return parsePasswordConfig(contents)
}

func atomicWrite(path string, contents []byte, mode os.FileMode) error {
	directory := filepath.Dir(path)
	if err := os.MkdirAll(directory, 0700); err != nil {
		return err
	}
	if info, err := os.Lstat(directory); err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("destination directory is not a real directory")
	}
	temporary, err := os.CreateTemp(directory, ".rom-uploader-*")
	if err != nil {
		return err
	}
	temporaryName := temporary.Name()
	ok := false
	defer func() {
		_ = temporary.Close()
		if !ok {
			_ = os.Remove(temporaryName)
		}
	}()
	if err := temporary.Chmod(mode); err != nil {
		return err
	}
	if _, err := temporary.Write(contents); err != nil {
		return err
	}
	if err := temporary.Sync(); err != nil {
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Rename(temporaryName, path); err != nil {
		return err
	}
	directoryHandle, err := os.Open(directory)
	if err != nil {
		return err
	}
	defer directoryHandle.Close()
	if err := directoryHandle.Sync(); err != nil {
		return err
	}
	ok = true
	return nil
}

func readPassword(input io.Reader) (string, error) {
	reader := bufio.NewReader(io.LimitReader(input, maximumPasswordSize+2))
	line, err := reader.ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	password := strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r")
	if len(password) < minimumPasswordSize || len(password) > maximumPasswordSize {
		return "", fmt.Errorf("password must contain %d through %d bytes", minimumPasswordSize, maximumPasswordSize)
	}
	if strings.ContainsAny(password, "\r\n\x00") {
		return "", errors.New("password contains forbidden control characters")
	}
	return password, nil
}
