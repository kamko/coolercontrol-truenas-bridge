package bridge

import (
	"encoding/json"
	"fmt"
	"os"
	"time"
)

type Config struct {
	TrueNAS TrueNASConfig `json:"truenas"`
	Export  ExportConfig  `json:"export"`
	Polling PollingConfig `json:"polling"`
}

type TrueNASConfig struct {
	Host      string   `json:"host"`
	Username  string   `json:"username"`
	APIKey    string   `json:"api_key"`
	TLS       bool     `json:"tls"`
	TLSVerify bool     `json:"tls_verify"`
	DiskNames []string `json:"disk_names"`
}

type ExportConfig struct {
	Directory       string `json:"directory"`
	WritePerDisk    bool   `json:"write_per_disk"`
	WriteStatusJSON bool   `json:"write_status_json"`
}

type PollingConfig struct {
	PollInterval       time.Duration `json:"-"`
	ConnectTimeout     time.Duration `json:"-"`
	StaleAfter         time.Duration `json:"-"`
	FailsafeTemperature float64       `json:"failsafe_temperature_c"`

	PollIntervalSeconds   int `json:"poll_interval_seconds"`
	ConnectTimeoutSeconds int `json:"connect_timeout_seconds"`
	StaleAfterSeconds     int `json:"stale_after_seconds"`
}

func LoadConfig(path string) (Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Config{}, err
	}

	var config Config
	if err := json.Unmarshal(data, &config); err != nil {
		return Config{}, err
	}

	applyDefaults(&config)
	if err := validate(config); err != nil {
		return Config{}, err
	}
	return config, nil
}

func applyDefaults(config *Config) {
	if config.Export.Directory == "" {
		config.Export.Directory = "/var/lib/truenas-coolercontrol-sensors"
	}
	if config.Polling.PollIntervalSeconds == 0 {
		config.Polling.PollIntervalSeconds = 300
	}
	if config.Polling.ConnectTimeoutSeconds == 0 {
		config.Polling.ConnectTimeoutSeconds = 15
	}
	if config.Polling.StaleAfterSeconds == 0 {
		config.Polling.StaleAfterSeconds = 900
	}
	if config.Polling.FailsafeTemperature == 0 {
		config.Polling.FailsafeTemperature = 55
	}

	config.Polling.PollInterval = time.Duration(config.Polling.PollIntervalSeconds) * time.Second
	config.Polling.ConnectTimeout = time.Duration(config.Polling.ConnectTimeoutSeconds) * time.Second
	config.Polling.StaleAfter = time.Duration(config.Polling.StaleAfterSeconds) * time.Second
}

func validate(config Config) error {
	if config.TrueNAS.Host == "" {
		return fmt.Errorf("truenas.host is required")
	}
	if config.TrueNAS.APIKey == "" {
		return fmt.Errorf("truenas.api_key is required")
	}
	if config.Polling.PollInterval < time.Second {
		return fmt.Errorf("polling.poll_interval_seconds must be at least 1")
	}
	return nil
}
