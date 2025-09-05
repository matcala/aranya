# Aranya Netcat Example

This example demonstrates running Aranya as a CLI tool to test communication over an AQC (Aranya Quic Channel) in a netcat-like fashion.

## Overview

This example uses only two devices:
- **Owner**: The device that performs onboarding and channel creation.
- **Member**: The device that gets onboarded to the team and exchanges data over AQC.

## Configuration

The default policy has been updated to allow communication between these two device types for testing purposes – the owner role is not allowed to a) assign itself a netID, b) create an AQC channel, and c) be a peer of an AQC channel.

## Usage

This tool provides a simple way to test AQC communication patterns similar to how netcat is used for network testing.