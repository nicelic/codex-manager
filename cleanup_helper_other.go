//go:build !windows

package main

func launchExitCleanupHelper() error { return nil }

func runCleanupHelper(args []string) {}
