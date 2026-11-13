// A program importing every package of the Go SDK, so that the image's
// module proxy holds every module they need.
package main

import (
	_ "go.origincloud.eu/sdk/billing/v1beta1"
	_ "go.origincloud.eu/sdk/compute/v1beta1"
	_ "go.origincloud.eu/sdk/core"
	_ "go.origincloud.eu/sdk/dns/v1beta1"
	_ "go.origincloud.eu/sdk/iam/v1beta1"
	_ "go.origincloud.eu/sdk/location/v1beta1"
	_ "go.origincloud.eu/sdk/longrunning/v1beta1"
	_ "go.origincloud.eu/sdk/network/v1beta1"
	_ "go.origincloud.eu/sdk/observability/v1beta1"
	_ "go.origincloud.eu/sdk/pvs/v1beta1"
	_ "go.origincloud.eu/sdk/resourcemanager/v1beta1"
	_ "go.origincloud.eu/sdk/sts/v1beta1"
	_ "go.origincloud.eu/sdk/support/v1beta1"
)

func main() {}
