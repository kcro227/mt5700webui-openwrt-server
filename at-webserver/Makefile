include $(TOPDIR)/rules.mk

PKG_NAME:=at-webserver
PKG_VERSION:=1.0
PKG_RELEASE:=16

PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)

include $(INCLUDE_DIR)/package.mk

define Package/at-webserver
  SECTION:=net
  CATEGORY:=Network
  TITLE:=AT Command WebSocket Server Rust Implementation
  PKGARCH:=all
endef

define Package/at-webserver/description
  	WebSocket server for AT command communication with web interface
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	$(CP) ./srcs/* $(PKG_BUILD_DIR)/
endef

# 确定 Rust 目标架构
ifeq ($(ARCH),aarch64)
  RUST_TARGET:=aarch64-unknown-linux-musl
else ifeq ($(ARCH),arm)
  ifeq ($(CONFIG_arm_v7),y)
    RUST_TARGET:=armv7-unknown-linux-musleabihf
  else
    RUST_TARGET:=arm-unknown-linux-musleabi
  endif
else ifeq ($(ARCH),x86_64)
  RUST_TARGET:=x86_64-unknown-linux-musl
endif

CARGO_ARGS:=--target=$(RUST_TARGET)
CARGO_ENV:=CARGO_TARGET_DIR=$(PKG_BUILD_DIR)/target \
           CC=$(TARGET_CC) \
           CXX=$(TARGET_CXX) \
           AR=$(TARGET_AR) \
           RUSTFLAGS="-C linker=$(TARGET_CC)" \
           PKG_CONFIG_ALLOW_CROSS=1

define Build/Compile
	(cd $(PKG_BUILD_DIR); \
		$(CARGO_ENV) cargo build $(CARGO_ARGS) --release )
endef

define Package/at-webserver/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/target/$(RUST_TARGET)/release/atserver $(1)/usr/bin/
	
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/etc/init.d/at-webserver $(1)/etc/init.d/
	
	$(INSTALL_DIR) $(1)/etc/config
	$(INSTALL_CONF) ./files/etc/config/at-webserver $(1)/etc/config/
	
	$(INSTALL_DIR) $(1)/www/5700
	$(CP) -r ./files/www/5700/* $(1)/www/5700/
	
	$(INSTALL_DIR) $(1)/www/cgi-bin
	$(INSTALL_BIN) ./files/www/cgi-bin/at-ws-info $(1)/www/cgi-bin/
	$(INSTALL_BIN) ./files/www/cgi-bin/at-log-clear $(1)/www/cgi-bin/
endef

define Package/at-webserver/conffiles
/etc/config/at-webserver
endef

$(eval $(call BuildPackage,at-webserver))

