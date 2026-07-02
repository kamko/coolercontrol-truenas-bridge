package bridge

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"nhooyr.io/websocket"
	"nhooyr.io/websocket/wsjson"
)

type TrueNASClient struct {
	config  TrueNASConfig
	timeout time.Duration
	nextID  int64
}

type rpcRequest struct {
	JSONRPC string `json:"jsonrpc"`
	ID      int64  `json:"id"`
	Method  string `json:"method"`
	Params  any    `json:"params"`
}

type rpcResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      int64           `json:"id"`
	Result  json.RawMessage `json:"result"`
	Error   *rpcError       `json:"error"`
}

type rpcError struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data"`
}

func NewTrueNASClient(config TrueNASConfig, timeout time.Duration) *TrueNASClient {
	return &TrueNASClient{
		config:  config,
		timeout: timeout,
		nextID:  1,
	}
}

func (c *TrueNASClient) DiskTemperatures(ctx context.Context) (map[string]float64, error) {
	ctx, cancel := context.WithTimeout(ctx, c.timeout)
	defer cancel()

	conn, err := c.dial(ctx)
	if err != nil {
		return nil, err
	}
	defer conn.Close(websocket.StatusNormalClosure, "done")

	if err := c.login(ctx, conn); err != nil {
		return nil, err
	}

	result, err := c.call(ctx, conn, "disk.temperatures", []any{c.config.DiskNames, false})
	if err != nil {
		return nil, err
	}

	var raw map[string]any
	if err := json.Unmarshal(result, &raw); err != nil {
		return nil, fmt.Errorf("decode disk.temperatures result: %w", err)
	}

	temperatures := NormalizeTemperatures(raw)
	if len(temperatures) == 0 {
		return nil, fmt.Errorf("TrueNAS returned no usable disk temperatures")
	}
	return temperatures, nil
}

func (c *TrueNASClient) dial(ctx context.Context) (*websocket.Conn, error) {
	options := &websocket.DialOptions{}
	if c.config.TLS && !c.config.TLSVerify {
		options.HTTPClient = &http.Client{
			Transport: &http.Transport{
				TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, //nolint:gosec
			},
		}
	}

	conn, _, err := websocket.Dial(ctx, c.websocketURL(), options)
	if err != nil {
		return nil, fmt.Errorf("connect TrueNAS WebSocket: %w", err)
	}
	return conn, nil
}

func (c *TrueNASClient) websocketURL() string {
	scheme := "ws"
	if c.config.TLS {
		scheme = "wss"
	}
	return fmt.Sprintf("%s://%s/api/current", scheme, c.config.Host)
}

func (c *TrueNASClient) login(ctx context.Context, conn *websocket.Conn) error {
	if c.config.Username != "" {
		result, err := c.call(ctx, conn, "auth.login_ex", []any{
			map[string]any{
				"mechanism": "API_KEY_PLAIN",
				"username":  c.config.Username,
				"api_key":   c.config.APIKey,
				"login_options": map[string]any{
					"user_info":       false,
					"reconnect_token": false,
				},
			},
		})
		if err != nil {
			return err
		}

		var response struct {
			ResponseType string `json:"response_type"`
		}
		if err := json.Unmarshal(result, &response); err != nil {
			return fmt.Errorf("decode auth.login_ex result: %w", err)
		}
		if response.ResponseType != "SUCCESS" {
			return fmt.Errorf("TrueNAS auth.login_ex failed with response_type=%q", response.ResponseType)
		}
		return nil
	}

	result, err := c.call(ctx, conn, "auth.login_with_api_key", []any{c.config.APIKey})
	if err != nil {
		return err
	}

	var ok bool
	if err := json.Unmarshal(result, &ok); err != nil {
		return fmt.Errorf("decode auth.login_with_api_key result: %w", err)
	}
	if !ok {
		return fmt.Errorf("TrueNAS legacy API key authentication failed")
	}
	return nil
}

func (c *TrueNASClient) call(ctx context.Context, conn *websocket.Conn, method string, params any) (json.RawMessage, error) {
	id := c.nextID
	c.nextID++

	request := rpcRequest{
		JSONRPC: "2.0",
		ID:      id,
		Method:  method,
		Params:  params,
	}
	if err := wsjson.Write(ctx, conn, request); err != nil {
		return nil, fmt.Errorf("send %s: %w", method, err)
	}

	for {
		var response rpcResponse
		if err := wsjson.Read(ctx, conn, &response); err != nil {
			return nil, fmt.Errorf("read %s response: %w", method, err)
		}
		if response.ID != id {
			continue
		}
		if response.Error != nil {
			return nil, fmt.Errorf("%s failed: %s (%d)", method, response.Error.Message, response.Error.Code)
		}
		return response.Result, nil
	}
}
