package bridge

import (
	"os"
	"path/filepath"
)

func WriteSample(config ExportConfig, sample TemperatureSample, failsafeTemperatureC float64) error {
	if err := os.MkdirAll(config.Directory, 0o755); err != nil {
		return err
	}

	hddMax := sample.HDDMaxC()
	if hddMax != hddMax {
		hddMax = failsafeTemperatureC
	}

	if err := AtomicWriteText(filepath.Join(config.Directory, "hdd_max_temp"), MillicelsiusText(hddMax)); err != nil {
		return err
	}

	if config.WritePerDisk {
		diskDir := filepath.Join(config.Directory, "disks")
		if err := os.MkdirAll(diskDir, 0o755); err != nil {
			return err
		}
		for diskName, temp := range sample.TemperaturesC {
			path := filepath.Join(diskDir, SafeSensorName(diskName)+"_temp")
			if err := AtomicWriteText(path, MillicelsiusText(temp)); err != nil {
				return err
			}
		}
	}

	if config.WriteStatusJSON {
		if err := AtomicWriteText(filepath.Join(config.Directory, "status.json"), sample.JSON()+"\n"); err != nil {
			return err
		}
	}

	return nil
}

func AtomicWriteText(path string, content string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}

	tmp, err := os.CreateTemp(filepath.Dir(path), "."+filepath.Base(path)+".")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()

	_, writeErr := tmp.WriteString(content)
	syncErr := tmp.Sync()
	closeErr := tmp.Close()

	if writeErr != nil || syncErr != nil || closeErr != nil {
		_ = os.Remove(tmpName)
		if writeErr != nil {
			return writeErr
		}
		if syncErr != nil {
			return syncErr
		}
		return closeErr
	}

	return os.Rename(tmpName, path)
}
