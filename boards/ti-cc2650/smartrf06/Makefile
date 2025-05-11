# Makefile for building the tock kernel for the CC2650-SmartRF06

export NEWLIB_INC_PATH ?= /usr/arm-none-eabi/include

# Decide on kernel/apps flash and RAM division
export KERNEL_FLASH_LEN ?= 0x13000
export APPS_FLASH_BEG := $(KERNEL_FLASH_LEN)
export APPS_RAM_BEG := 0x20003000

TOCK_BOARD=ti-cc2650-smartrf06
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-smartrf06
OPENOCD ?= openocd

include ../../Makefile.common
include ../common/Makefile.common

HENI_PROGRAM := $(HENI) node program dk

### Flashing kernel only
.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin
	$(OPENOCD) -f flash-kernel.openocd

### Flashing kernel + an app
.PHONY: app-%-flash
app-%-flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.hex
	$(HENI_PROGRAM) $<
